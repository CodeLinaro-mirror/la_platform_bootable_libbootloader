// Copyright 2024, The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Gbl AVB operations.

use crate::{
    gbl_avb::state::{BootStateColor, KeyValidationStatus},
    GblOps,
};
use abr::SlotIndex;
use arrayvec::ArrayString;
use avb::{
    cert_validate_vbmeta_public_key, CertOps, CertPermanentAttributes, IoError, IoResult,
    Ops as AvbOps, PublicKeyForPartitionInfo, SlotVerifyData, SHA256_DIGEST_SIZE,
    SHA512_DIGEST_SIZE,
};
use core::fmt::Write;
use core::{
    cmp::{max, min},
    ffi::CStr,
};
use liberror::Error;
use safemath::SafeNum;
use uuid::Uuid;

#[cfg(feature = "gbl_dev")]
use crate::gbl_println;
#[cfg(feature = "gbl_dev")]
use core::fmt::Debug;

/// The digest key in commandline provided by libavb.
pub const AVB_DIGEST_KEY: &str = "androidboot.vbmeta.digest";

// AVB cert tracks versions for the PIK and PSK; PRK cannot be changed so has no version info.
const AVB_CERT_NUM_KEY_VERSIONS: usize = 2;

/// Implements avb ops callbacks for [GblOps].
pub struct GblAvbOps<'a, 'b, T> {
    /// The underlying [GblOps].
    pub gbl_ops: &'b mut T,
    slot: Option<SlotIndex>,
    /// Slotless partitions pre-loaded by the implementation. Provided to avoid redundant IO.
    preloaded_partitions: &'a [(&'a str, &'a [u8])],
    /// Used for storing key versions to be set (location, version).
    ///
    /// These will initially be `None`, but if using the cert extensions they will be updated during
    /// verification. These values will not be automatically persisted to disk because whether to do
    /// so depends on other factors such as slot success state; it's up to the user to persist them
    /// post-verification if needed.
    // If `array_map` is imported in the future, consider switching to it.
    pub key_versions: [Option<(usize, u64)>; AVB_CERT_NUM_KEY_VERSIONS],
    /// True to use the AVB cert extensions.
    use_cert: bool,
    /// Avb public key validation status reported by validate_vbmeta_public_key.
    /// https://source.android.com/docs/security/features/verifiedboot/boot-flow#locked-devices-with-custom-root-of-trust
    key_validation_status: Option<KeyValidationStatus>,
}

impl<'a, 'b, 'p, 'q, T: GblOps<'p, 'q>> GblAvbOps<'a, 'b, T> {
    /// Creates a new [GblAvbOps].
    pub fn new(
        gbl_ops: &'b mut T,
        slot: Option<SlotIndex>,
        preloaded_partitions: &'a [(&'a str, &'a [u8])],
        use_cert: bool,
    ) -> Self {
        Self {
            gbl_ops,
            slot,
            preloaded_partitions,
            key_versions: [None; AVB_CERT_NUM_KEY_VERSIONS],
            use_cert,
            key_validation_status: None,
        }
    }

    /// Returns the size of a partition.
    ///
    /// This will only consider the [GblOps] partitions. To include preloaded partitions as well,
    /// use [AvbOps::get_size_of_partition].
    fn partition_size(&mut self, partition: &str) -> IoResult<u64> {
        self.gbl_ops.partition_size(partition).or(Err(IoError::Io))?.ok_or(IoError::NoSuchPartition)
    }

    /// Allowes implementation side to handle verification result.
    pub fn handle_verification_result(
        &mut self,
        slot_verify: Option<&SlotVerifyData>,
        color: BootStateColor,
        digest: Option<&str>,
    ) -> IoResult<()> {
        // The Android build system automatically generates only the main vbmeta, but also allows
        // to have separate chained partitions like vbmeta_system (for system, product, system_ext,
        // etc.) or vbmeta_vendor (for vendor).
        // https://android.googlesource.com/platform/external/avb/+/master/README.md#build-system-integration
        //
        // It may also integrate chained vbmeta into system level metadata partitions such as boot
        // or init_boot, so they can be updated separately.
        // https://android.googlesource.com/platform/external/avb/+/master/README.md#gki-2_0-integration
        //
        // Custom chained partitions are also supported by the Android build system, but we expect
        // OEMs to follow about the same pattern.
        // https://android-review.googlesource.com/q/Id671e2c3aee9ada90256381cce432927df03169b
        let (
            boot_os_version,
            boot_security_patch,
            system_os_version,
            system_security_patch,
            vendor_os_version,
            vendor_security_patch,
        ) = match slot_verify {
            Some(slot_verify) => {
                let mut vbmeta = None;
                let mut vbmeta_boot = None;
                let mut vbmeta_system = None;
                let mut vbmeta_vendor = None;

                for data in slot_verify.vbmeta_data() {
                    match data.partition_name().to_str().unwrap_or_default() {
                        "vbmeta" => vbmeta = Some(data),
                        "boot" => vbmeta_boot = Some(data),
                        "vbmeta_system" => vbmeta_system = Some(data),
                        "vbmeta_vendor" => vbmeta_vendor = Some(data),
                        _ => {}
                    }
                }

                let data = vbmeta.ok_or(IoError::NoSuchPartition)?;
                let boot_data = vbmeta_boot.unwrap_or(data);
                let system_data = vbmeta_system.unwrap_or(data);
                let vendor_data = vbmeta_vendor.unwrap_or(data);

                (
                    boot_data.get_property_value("com.android.build.boot.os_version"),
                    boot_data.get_property_value("com.android.build.boot.security_patch"),
                    system_data.get_property_value("com.android.build.system.os_version"),
                    system_data.get_property_value("com.android.build.system.security_patch"),
                    vendor_data.get_property_value("com.android.build.vendor.os_version"),
                    vendor_data.get_property_value("com.android.build.vendor.security_patch"),
                )
            }
            None => (None, None, None, None, None, None),
        };

        // Convert digest rust string to null-terminated string by copying it into separate buffer.
        let mut digest_buffer = ArrayString::<{ 2 * SHA512_DIGEST_SIZE + 1 }>::new();
        let digest_cstr = match digest {
            Some(digest) => {
                write!(digest_buffer, "{}\0", digest).or(Err(IoError::InvalidValueSize))?;
                Some(
                    CStr::from_bytes_until_nul(digest_buffer.as_bytes())
                        .or(Err(IoError::InvalidValueSize))?,
                )
            }
            None => None,
        };

        self.gbl_ops.avb_handle_verification_result(
            color,
            digest_cstr,
            boot_os_version,
            boot_security_patch,
            system_os_version,
            system_security_patch,
            vendor_os_version,
            vendor_security_patch,
        )
    }

    /// Get vbmeta public key validation status reported by validate_vbmeta_public_key.
    pub fn key_validation_status(&self) -> IoResult<KeyValidationStatus> {
        self.key_validation_status.ok_or(IoError::NotImplemented)
    }

    /// For dev builds only, transforms unimplemented ops into default behavior.
    ///
    /// # Args
    /// * `result`: the result provided by the real [GblOps].
    /// * `fallback`: the result to fall back to if the op is unimplemented.
    /// * `log_name`: if the fallback value is used, some logs will be printed
    ///               containing this name and the fallback value.
    ///
    /// # Returns
    /// If `result` is `Err(IoError::NotImplemented)`, returns `fallback`.
    /// Otherwise returns `result` unchanged.
    #[cfg(feature = "gbl_dev")]
    fn with_dev_fallback<R: Debug>(
        &mut self,
        result: IoResult<R>,
        fallback: IoResult<R>,
        log_name: &str,
    ) -> IoResult<R> {
        match result {
            Err(IoError::NotImplemented) => {
                gbl_println!(
                    self.gbl_ops,
                    "AVB {} unimplemented, defaulting to {:?}",
                    log_name,
                    fallback
                );
                fallback
            }
            _ => result,
        }
    }

    /// For dev builds only, returns true if both cert ops return
    /// [IoError::NotImplemented].
    #[cfg(feature = "gbl_dev")]
    fn cert_ops_not_implemented(&mut self) -> bool {
        let mut attributes = CertPermanentAttributes {
            version: 0,
            product_root_public_key: [0u8; 1032],
            product_id: [0u8; 16],
        };
        self.read_permanent_attributes(&mut attributes) == Err(IoError::NotImplemented)
            && self.read_permanent_attributes_hash() == Err(IoError::NotImplemented)
    }
}

/// A helper function for converting `CStr` to `str`
fn cstr_to_str<E>(s: &CStr, err: E) -> Result<&str, E> {
    Ok(s.to_str().or(Err(err))?)
}

/// A helper function to split partition into base name and slot index
fn split_slotted(partition: &str) -> Result<(&str, SlotIndex), Error> {
    // Attempt to split on the last underscore
    let (partition_name, suffix) = partition.rsplit_once('_').ok_or(Error::InvalidInput)?;

    // Ensure suffix has exactly one character
    if suffix.len() != 1 {
        return Err(Error::InvalidInput);
    }

    // Convert that single character into a SlotIndex
    let slot_char = suffix.chars().next().unwrap();
    let slot = slot_char.try_into().map_err(|_| Error::InvalidInput)?;

    Ok((partition_name, slot))
}

/// # Lifetimes
/// * `'a`: preloaded data lifetime
/// * `'b`: [GblOps] partition lifetime
impl<'a, 'b, 'p, 'q, T: GblOps<'p, 'q>> AvbOps<'a> for GblAvbOps<'a, 'b, T> {
    fn read_from_partition(
        &mut self,
        partition: &CStr,
        offset: i64,
        buffer: &mut [u8],
    ) -> IoResult<usize> {
        let part_str = cstr_to_str(partition, IoError::NoSuchPartition)?;
        let partition_size = SafeNum::from(self.partition_size(part_str)?);
        let read_off = match offset < 0 {
            true => partition_size - offset.abs(),
            _ => SafeNum::from(offset),
        };
        let read_sz = partition_size - read_off;
        let read_off = read_off.try_into().or(Err(IoError::RangeOutsidePartition))?;
        let read_sz =
            min(buffer.len(), read_sz.try_into().or(Err(IoError::RangeOutsidePartition))?);
        self.gbl_ops.read_from_partition_sync(part_str, read_off, &mut buffer[..read_sz]).map_err(
            |e| match e {
                Error::NotFound => IoError::NoSuchPartition,
                Error::ArithmeticOverflow(_) => IoError::RangeOutsidePartition,
                _ => IoError::Io,
            },
        )?;
        Ok(read_sz)
    }

    fn get_preloaded_partition(&mut self, partition: &CStr) -> IoResult<&'a [u8]> {
        let part_str = cstr_to_str(partition, IoError::NotImplemented)?;

        let partition_name = match self.slot {
            // Extract partition slot and ensure it's matched.
            Some(slot) => {
                let (partition_name, partition_slot) =
                    split_slotted(part_str).map_err(|_| IoError::NotImplemented)?;

                if partition_slot != slot {
                    return Err(IoError::NotImplemented);
                }

                partition_name
            }
            _ => part_str,
        };

        self.preloaded_partitions
            .iter()
            .find(|(name, _)| *name == partition_name)
            .map(|(_, data)| *data)
            .ok_or_else(|| IoError::NotImplemented)
    }

    fn validate_vbmeta_public_key(
        &mut self,
        public_key: &[u8],
        public_key_metadata: Option<&[u8]>,
    ) -> IoResult<bool> {
        let result = if self.use_cert {
            let result = cert_validate_vbmeta_public_key(self, public_key, public_key_metadata)
                .map(|ret| match ret {
                    true => KeyValidationStatus::Valid,
                    false => KeyValidationStatus::Invalid,
                });

            // `cert_validate_vbmeta_public_key` is a little trickier to
            // identify the not-implemented case, because
            // [IoError::NotImplemented] is only returned directly from ops
            // callbacks; top-level libavb APIs like this will end up converting
            // it to [IoError::Io] instead.
            //
            // To work around this, we make an additional call to each cert op
            // callback to check whether it's implemented or not.
            #[cfg(feature = "gbl_dev")]
            let result = match result {
                Err(IoError::Io) if self.cert_ops_not_implemented() => Err(IoError::NotImplemented),
                _ => result,
            };

            result
        } else {
            self.gbl_ops.avb_validate_vbmeta_public_key(public_key, public_key_metadata)
        };

        // On dev boards fall back to `Invalid`, which indicates that the
        // boot image was not signed but will still allow booting on
        // unlocked boards.
        #[cfg(feature = "gbl_dev")]
        let result =
            self.with_dev_fallback(result, Ok(KeyValidationStatus::Invalid), "validate vbmeta key");

        let status = result?;
        self.key_validation_status = Some(status);
        Ok(matches!(status, KeyValidationStatus::Valid | KeyValidationStatus::ValidCustomKey))
    }

    fn read_rollback_index(&mut self, rollback_index_location: usize) -> IoResult<u64> {
        let result = self.gbl_ops.avb_read_rollback_index(rollback_index_location);

        // On dev boards always read 0, which allows any version to boot.
        #[cfg(feature = "gbl_dev")]
        let result = self.with_dev_fallback(result, Ok(0), "read rollback index");

        result
    }

    fn write_rollback_index(&mut self, rollback_index_location: usize, index: u64) -> IoResult<()> {
        let result = self.gbl_ops.avb_write_rollback_index(rollback_index_location, index);

        // On dev boards writing rollback is a no-op, always return success.
        #[cfg(feature = "gbl_dev")]
        let result = self.with_dev_fallback(result, Ok(()), "write rollback index");

        result
    }

    fn read_is_device_unlocked(&mut self) -> IoResult<bool> {
        let result = self.gbl_ops.avb_read_is_device_unlocked();

        // On dev boards default to unlocked, which allows boot to succeed.
        #[cfg(feature = "gbl_dev")]
        let result = self.with_dev_fallback(result, Ok(true), "read device unlocked");

        result
    }

    fn get_unique_guid_for_partition(&mut self, partition: &CStr) -> IoResult<Uuid> {
        // The ops is only used to check that a partition exists. GUID is not used.
        self.partition_size(cstr_to_str(partition, IoError::NoSuchPartition)?)?;
        Ok(Uuid::nil())
    }

    fn get_size_of_partition(&mut self, partition: &CStr) -> IoResult<u64> {
        match self.get_preloaded_partition(partition) {
            Ok(img) => Ok(img.len().try_into().unwrap()),
            _ => {
                let part_str = cstr_to_str(partition, IoError::NoSuchPartition)?;
                self.partition_size(part_str)
            }
        }
    }

    fn read_persistent_value(&mut self, name: &CStr, value: &mut [u8]) -> IoResult<usize> {
        let result = self.gbl_ops.avb_read_persistent_value(name, value);

        // On dev boards default to no persistent values. libavb will handle
        // this as a verification error and still allow booting when unlocked.
        #[cfg(feature = "gbl_dev")]
        let result =
            self.with_dev_fallback(result, Err(IoError::NoSuchValue), "read persistent value");

        result
    }

    fn write_persistent_value(&mut self, name: &CStr, value: &[u8]) -> IoResult<()> {
        let result = self.gbl_ops.avb_write_persistent_value(name, value);

        // On dev boards default to no persistent values. libavb will handle
        // this as a verification error and still allow booting when unlocked.
        #[cfg(feature = "gbl_dev")]
        let result =
            self.with_dev_fallback(result, Err(IoError::NoSuchValue), "write persistent value");

        result
    }

    fn erase_persistent_value(&mut self, name: &CStr) -> IoResult<()> {
        let result = self.gbl_ops.avb_erase_persistent_value(name);

        // On dev boards default to no persistent values. libavb will handle
        // this as a verification error and still allow booting when unlocked.
        #[cfg(feature = "gbl_dev")]
        let result =
            self.with_dev_fallback(result, Err(IoError::NoSuchValue), "erase persistent value");

        result
    }

    fn validate_public_key_for_partition(
        &mut self,
        _partition: &CStr,
        _public_key: &[u8],
        _public_key_metadata: Option<&[u8]>,
    ) -> IoResult<PublicKeyForPartitionInfo> {
        // Not needed yet; eventually we will plumb this through [GblOps].
        unreachable!();
    }

    fn cert_ops(&mut self) -> Option<&mut dyn CertOps> {
        match self.use_cert {
            true => Some(self),
            false => None,
        }
    }
}

/// [GblAvbOps] always implements [CertOps], but it's only used if `use_cert` is set.
impl<'a, 'b, T: GblOps<'a, 'b>> CertOps for GblAvbOps<'_, '_, T> {
    fn read_permanent_attributes(
        &mut self,
        attributes: &mut CertPermanentAttributes,
    ) -> IoResult<()> {
        self.gbl_ops.avb_cert_read_permanent_attributes(attributes)
    }

    fn read_permanent_attributes_hash(&mut self) -> IoResult<[u8; SHA256_DIGEST_SIZE]> {
        self.gbl_ops.avb_cert_read_permanent_attributes_hash()
    }

    fn set_key_version(&mut self, rollback_index_location: usize, key_version: u64) {
        // Checks if there is already an allocated slot for this location.
        let existing = self
            .key_versions
            .iter_mut()
            .find_map(|v| v.as_mut().filter(|(loc, _)| *loc == rollback_index_location));
        match existing {
            Some((_, val)) => *val = max(*val, key_version),
            _ => {
                // Finds an empty slot and stores the rollback index.
                *self
                    .key_versions
                    .iter_mut()
                    .find(|v| v.is_none())
                    .expect("Ran out of key version slots") =
                    Some((rollback_index_location, key_version))
            }
        }
    }

    fn get_random(&mut self, _: &mut [u8]) -> IoResult<()> {
        // Not needed yet; eventually we will plumb this through [GblOps].
        unimplemented!()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        ops::test::{FakeGblOps, FakeGblOpsStorage},
        tests::{testdata, TEST_PERMANENT_ATTRIBUTES_HASH_PATH, TEST_PERMANENT_ATTRIBUTES_PATH},
    };
    use avb::{CERT_PIK_VERSION_LOCATION, CERT_PSK_VERSION_LOCATION};
    use zerocopy::FromBytes;

    const TEST_CERT_PUBLIC_KEY_PATH: &str = "testkey_cert_psk.bin";
    const TEST_CERT_METADATA_PATH: &str = "cert_metadata.bin";

    // Returns test data consisting of `size` incrementing bytes (0-255 repeating).
    fn test_data(size: usize) -> Vec<u8> {
        let mut data = vec![0u8; size];
        for index in 0..data.len() {
            data[index] = index as u8;
        }
        data
    }

    #[test]
    fn read_from_partition_positive_off() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"test_part", test_data(512));

        let mut gbl_ops = FakeGblOps::new(&storage);
        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);

        // Positive offset.
        let mut out = [0u8; 4];
        assert_eq!(avb_ops.read_from_partition(c"test_part", 1, &mut out[..]), Ok(4));
        assert_eq!(out, [1, 2, 3, 4]);
    }

    #[test]
    fn read_from_partition_negative_off() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"test_part", test_data(512));

        let mut gbl_ops = FakeGblOps::new(&storage);
        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);

        // Negative offset should wrap from the end
        let mut out = [0u8; 6];
        assert_eq!(avb_ops.read_from_partition(c"test_part", -6, &mut out[..]), Ok(6));
        assert_eq!(out, [0xFA, 0xFB, 0xFC, 0xFD, 0xFE, 0xFF]);
    }

    #[test]
    fn read_from_partition_partial_read() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"test_part", test_data(512));

        let mut gbl_ops = FakeGblOps::new(&storage);
        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);

        // Reading past the end of the partition should truncate.
        let mut out = [0u8; 6];
        assert_eq!(avb_ops.read_from_partition(c"test_part", -3, &mut out[..]), Ok(3));
        assert_eq!(out, [0xFD, 0xFE, 0xFF, 0, 0, 0]);
    }

    #[test]
    fn read_from_partition_out_of_bounds() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"test_part", test_data(512));

        let mut gbl_ops = FakeGblOps::new(&storage);
        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);

        // Reads starting out of bounds should fail.
        let mut out = [0u8; 4];
        assert_eq!(
            avb_ops.read_from_partition(c"test_part", 513, &mut out[..]),
            Err(IoError::RangeOutsidePartition)
        );
        assert_eq!(
            avb_ops.read_from_partition(c"test_part", -513, &mut out[..]),
            Err(IoError::RangeOutsidePartition)
        );
    }

    #[test]
    fn read_from_partition_unknown_part() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);

        let mut out = [0u8; 4];
        assert_eq!(
            avb_ops.read_from_partition(c"unknown_part", 0, &mut out[..]),
            Err(IoError::NoSuchPartition)
        );
    }

    /// Helper function to test reading pre-loaded partitions.
    fn test_read_preloaded_partition(
        preloaded_partition: &str,
        slot: Option<SlotIndex>,
        partition_to_read: &CStr,
        expect_success: bool,
    ) {
        let mut gbl_ops = FakeGblOps::new(&[]);

        let data = &test_data(512);
        let slice = &data[..];
        let preloaded = [(preloaded_partition, slice)];
        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, slot, &preloaded, false);

        match expect_success {
            true => {
                assert_eq!(
                    avb_ops.get_size_of_partition(partition_to_read),
                    Ok(data.len().try_into().unwrap())
                );
                assert_eq!(avb_ops.get_preloaded_partition(partition_to_read), Ok(slice));
            }
            false => {
                assert_eq!(
                    avb_ops.get_preloaded_partition(partition_to_read),
                    Err(IoError::NotImplemented),
                );
            }
        }
    }

    #[test]
    fn read_from_preloaded_a_partition() {
        test_read_preloaded_partition(
            "test_partition",
            Some(SlotIndex::A),
            c"test_partition_a",
            true,
        );
    }

    #[test]
    fn read_from_preloaded_b_partition() {
        test_read_preloaded_partition(
            "test_partition",
            Some(SlotIndex::B),
            c"test_partition_b",
            true,
        );
    }

    #[test]
    fn read_from_preloaded_r_partition() {
        test_read_preloaded_partition(
            "test_partition",
            Some(SlotIndex::R),
            c"test_partition_r",
            true,
        );
    }

    #[test]
    fn read_from_preloaded_slotless_partition() {
        test_read_preloaded_partition("test_partition", None, c"test_partition", true);
    }

    #[test]
    fn read_from_preloaded_partition_wrong_slot() {
        // Ops are slotless but _a is used, so cannot read.
        test_read_preloaded_partition("test_partition", None, c"test_partition_a", false);

        // Ops are using A slot but slotless is getting read, so cannot read.
        test_read_preloaded_partition(
            "test_partition",
            Some(SlotIndex::A),
            c"test_partition",
            false,
        );

        // Ops are using A slot but _b is getting read, so cannot read.
        test_read_preloaded_partition(
            "test_partition",
            Some(SlotIndex::A),
            c"test_partition_b",
            false,
        );
    }

    #[test]
    fn set_key_version_default() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        let avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);

        assert_eq!(avb_ops.key_versions, [None, None]);
    }

    #[test]
    fn set_key_version_once() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);

        avb_ops.set_key_version(5, 10);
        assert_eq!(avb_ops.key_versions, [Some((5, 10)), None]);
    }

    #[test]
    fn set_key_version_twice() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);

        avb_ops.set_key_version(5, 10);
        avb_ops.set_key_version(20, 40);
        assert_eq!(avb_ops.key_versions, [Some((5, 10)), Some((20, 40))]);
    }

    #[test]
    fn set_key_version_overwrite() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);

        avb_ops.set_key_version(5, 10);
        avb_ops.set_key_version(20, 40);
        avb_ops.set_key_version(5, 100);
        assert_eq!(avb_ops.key_versions, [Some((5, 100)), Some((20, 40))]);
    }

    // AVB's key version callback cannot return an error, so if it fails we panic.
    //
    // It's possible we could stash the failure somewhere and check it later, but we'd have to be
    // very careful, as failing to check the status would be a security vulnerability. For now it's
    // safer to panic, and we only ever expect the PSK and PIK to have key versions.
    #[test]
    #[should_panic(expected = "Ran out of key version slots")]
    fn set_key_version_overflow() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);

        avb_ops.set_key_version(5, 10);
        avb_ops.set_key_version(20, 40);
        avb_ops.set_key_version(40, 100);
    }

    /// Returns `value` in a dev build, [IoError::NotImplemented] in a prod build.
    fn dev_only<T>(value: IoResult<T>) -> IoResult<T> {
        match cfg!(feature = "gbl_dev") {
            true => value,
            false => Err(IoError::NotImplemented),
        }
    }

    #[test]
    fn validate_vbmeta_public_key_valid() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_key_validation_status = Some(Ok(KeyValidationStatus::Valid));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        assert_eq!(avb_ops.validate_vbmeta_public_key(&[], None), Ok(true));
        assert_eq!(avb_ops.key_validation_status(), Ok(KeyValidationStatus::Valid));
    }

    #[test]
    fn validate_vbmeta_public_key_valid_custom_key() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_key_validation_status = Some(Ok(KeyValidationStatus::ValidCustomKey));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        assert_eq!(avb_ops.validate_vbmeta_public_key(&[], None), Ok(true));
        assert_eq!(avb_ops.key_validation_status(), Ok(KeyValidationStatus::ValidCustomKey));
    }

    #[test]
    fn validate_vbmeta_public_key_invalid() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_key_validation_status = Some(Ok(KeyValidationStatus::Invalid));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        assert_eq!(avb_ops.validate_vbmeta_public_key(&[], None), Ok(false));
        assert_eq!(avb_ops.key_validation_status(), Ok(KeyValidationStatus::Invalid));
    }

    #[test]
    fn validate_vbmeta_public_key_failed() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_key_validation_status = Some(Err(IoError::Io));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        assert_eq!(avb_ops.validate_vbmeta_public_key(&[], None), Err(IoError::Io));
        assert!(avb_ops.key_validation_status().is_err());
    }

    #[test]
    fn validate_vbmeta_public_key_not_implemented() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_key_validation_status = Some(Err(IoError::NotImplemented));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);

        // Dev should succeed but report invalid key, prod should fail.
        assert_eq!(avb_ops.validate_vbmeta_public_key(&[], None), dev_only(Ok(false)));
        assert_eq!(avb_ops.key_validation_status(), dev_only(Ok(KeyValidationStatus::Invalid)));
    }

    /// Creates a [FakeGblOps] with all the necessary configuration to
    /// successfully validate the vbmeta key using the libavb cert extension.
    ///
    /// # Returns
    /// A tuple containing:
    /// * the [FakeGblOps]
    /// * the corresponding vbmeta public key
    /// * the corresponding vbmeta public key metadata
    fn create_fake_gbl_ops_with_cert() -> (FakeGblOps<'static, 'static>, Vec<u8>, Vec<u8>) {
        let mut gbl_ops = FakeGblOps::new(&[]);

        // Cert verification requires both permanent attribute ops plus reading
        // rollback indices for the signing keys.
        gbl_ops.avb_ops.cert_permanent_attributes = Some(
            CertPermanentAttributes::read_from(&testdata(TEST_PERMANENT_ATTRIBUTES_PATH)).unwrap(),
        );
        gbl_ops.avb_ops.cert_permanent_attributes_hash =
            Some(testdata(TEST_PERMANENT_ATTRIBUTES_HASH_PATH).try_into().unwrap());
        gbl_ops.avb_ops.rollbacks.insert(CERT_PIK_VERSION_LOCATION, Ok(0));
        gbl_ops.avb_ops.rollbacks.insert(CERT_PSK_VERSION_LOCATION, Ok(0));

        (gbl_ops, testdata(TEST_CERT_PUBLIC_KEY_PATH), testdata(TEST_CERT_METADATA_PATH))
    }

    #[test]
    fn cert_validate_vbmeta_public_key_valid() {
        let (mut gbl_ops, public_key, metadata) = create_fake_gbl_ops_with_cert();

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], true);
        assert_eq!(avb_ops.validate_vbmeta_public_key(&public_key, Some(&metadata)), Ok(true));
        assert_eq!(avb_ops.key_validation_status(), Ok(KeyValidationStatus::Valid));
    }

    #[test]
    fn cert_validate_vbmeta_public_key_invalid() {
        let (mut gbl_ops, mut public_key, metadata) = create_fake_gbl_ops_with_cert();

        // Modify the public key so it no longer matches the perm attributes.
        public_key[0] ^= 0x01;

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], true);
        assert_eq!(avb_ops.validate_vbmeta_public_key(&public_key, Some(&metadata)), Ok(false));
        assert_eq!(avb_ops.key_validation_status(), Ok(KeyValidationStatus::Invalid));
    }

    #[test]
    fn cert_validate_vbmeta_public_key_failed() {
        let (mut gbl_ops, public_key, metadata) = create_fake_gbl_ops_with_cert();

        // Setting the fake perm attributes to `None` causes [IoError::Io].
        gbl_ops.avb_ops.cert_permanent_attributes = None;

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], true);
        assert_eq!(
            avb_ops.validate_vbmeta_public_key(&public_key, Some(&metadata)),
            Err(IoError::Io)
        );
        assert!(avb_ops.key_validation_status().is_err());
    }

    #[test]
    fn cert_validate_vbmeta_public_key_not_implemented() {
        // Start with regular [FakeGblOps] without cert backends so we can
        // make sure everything reports [IoError::NotImplemented].
        let mut gbl_ops = FakeGblOps::new(&[]);

        // Cert verification requires both permanent attribute ops plus reading
        // rollback indices for the signing keys.
        gbl_ops.avb_cert_read_permanent_attributes_not_implemented = true;
        gbl_ops.avb_cert_read_permanent_attributes_hash_not_implemented = true;
        gbl_ops.avb_ops.rollbacks.insert(CERT_PIK_VERSION_LOCATION, Err(IoError::NotImplemented));
        gbl_ops.avb_ops.rollbacks.insert(CERT_PSK_VERSION_LOCATION, Err(IoError::NotImplemented));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], true);
        // Dev should succeed but report invalid key, prod should fail.
        //
        // Because of the extra complications detecting the not-implemented case
        // for cert verification, in prod builds this ends up failing with
        // [IoError::Io] rather than [IoError::NotImplemented] like the other
        // ops do. This could cause some minor confusion for developers, but it
        // doesn't seem worth adding the workaround logic to prod builds.
        assert_eq!(
            avb_ops.validate_vbmeta_public_key(&[], None),
            match cfg!(feature = "gbl_dev") {
                true => Ok(false),
                false => Err(IoError::Io),
            }
        );
        assert_eq!(avb_ops.key_validation_status(), dev_only(Ok(KeyValidationStatus::Invalid)));
    }

    #[test]
    fn read_rollback_index_read_value() {
        const EXPECTED_INDEX: usize = 1;
        const EXPECTED_VALUE: u64 = 100;

        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_ops.rollbacks.insert(EXPECTED_INDEX, Ok(EXPECTED_VALUE));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        assert_eq!(avb_ops.read_rollback_index(EXPECTED_INDEX), Ok(EXPECTED_VALUE));
    }

    #[test]
    fn read_rollback_index_error_handled() {
        let mut gbl_ops = FakeGblOps::new(&[]);

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        assert_eq!(avb_ops.read_rollback_index(0), Err(IoError::Io));
    }

    #[test]
    fn read_rollback_index_not_implemented() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_ops.rollbacks.insert(0, Err(IoError::NotImplemented));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);

        // Dev should always return 0, prod should fail.
        assert_eq!(avb_ops.read_rollback_index(0), dev_only(Ok(0)));
    }

    #[test]
    fn write_rollback_index_write_value() {
        const EXPECTED_INDEX: usize = 1;
        const EXPECTED_VALUE: u64 = 100;

        let mut gbl_ops = FakeGblOps::new(&[]);

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        assert_eq!(avb_ops.write_rollback_index(EXPECTED_INDEX, EXPECTED_VALUE), Ok(()));
        assert_eq!(
            gbl_ops.avb_ops.rollbacks.get(&EXPECTED_INDEX),
            Some(Ok(EXPECTED_VALUE)).as_ref()
        );
    }

    #[test]
    fn write_rollback_index_error_handled() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_ops.rollbacks.insert(0, Err(IoError::Io));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        assert_eq!(avb_ops.write_rollback_index(0, 0), Err(IoError::Io));
    }

    #[test]
    fn write_rollback_index_not_implemented() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_ops.rollbacks.insert(0, Err(IoError::NotImplemented));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        // Dev should always succeed, prod should fail.
        assert_eq!(avb_ops.write_rollback_index(0, 0), dev_only(Ok(())));
    }

    #[test]
    fn read_is_device_unlocked_value_obtained() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_ops.unlock_state = Ok(true);

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);

        assert_eq!(avb_ops.read_is_device_unlocked(), Ok(true));
    }

    #[test]
    fn read_is_device_unlocked_error_handled() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_ops.unlock_state = Err(IoError::Io);

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        assert_eq!(avb_ops.read_is_device_unlocked(), Err(IoError::Io));
    }

    #[test]
    fn read_is_device_unlocked_not_implemented() {
        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_ops.unlock_state = Err(IoError::NotImplemented);

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        // Dev should report unlocked, prod should fail.
        assert_eq!(avb_ops.read_is_device_unlocked(), dev_only(Ok(true)));
    }

    #[test]
    fn read_persistent_value_success() {
        const EXPECTED_NAME: &CStr = c"test";
        const EXPECTED_VALUE: &[u8] = b"test";

        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_ops.add_persistent_value(EXPECTED_NAME.to_str().unwrap(), Ok(EXPECTED_VALUE));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        let mut buffer = [0u8; EXPECTED_VALUE.len()];
        assert_eq!(
            avb_ops.read_persistent_value(EXPECTED_NAME, &mut buffer),
            Ok(EXPECTED_VALUE.len())
        );
        assert_eq!(buffer, EXPECTED_VALUE);
    }

    #[test]
    fn read_persistent_value_error() {
        const EXPECTED_NAME: &CStr = c"test";

        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_ops.add_persistent_value(EXPECTED_NAME.to_str().unwrap(), Err(IoError::Io));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        let mut buffer = [0u8; 4];
        assert_eq!(avb_ops.read_persistent_value(EXPECTED_NAME, &mut buffer), Err(IoError::Io));
    }

    #[test]
    fn read_persistent_value_not_implemented() {
        const EXPECTED_NAME: &CStr = c"test";

        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops
            .avb_ops
            .add_persistent_value(EXPECTED_NAME.to_str().unwrap(), Err(IoError::NotImplemented));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        let mut buffer = [0u8; 0];
        // Dev should report no such value, prod should fail.
        assert_eq!(
            avb_ops.read_persistent_value(EXPECTED_NAME, &mut buffer),
            dev_only(Err(IoError::NoSuchValue))
        );
    }

    #[test]
    fn write_persistent_value_success() {
        const EXPECTED_NAME: &CStr = c"test";
        const EXPECTED_VALUE: &[u8] = b"test";

        let mut gbl_ops = FakeGblOps::new(&[]);

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        assert_eq!(avb_ops.write_persistent_value(EXPECTED_NAME, EXPECTED_VALUE), Ok(()));

        assert_eq!(
            gbl_ops.avb_ops.persistent_values.get(EXPECTED_NAME.to_str().unwrap()),
            Some(Ok(EXPECTED_VALUE.to_vec())).as_ref()
        );
    }

    #[test]
    fn write_persistent_value_error() {
        const EXPECTED_NAME: &CStr = c"test";
        const EXPECTED_VALUE: &[u8] = b"test";

        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_ops.add_persistent_value(EXPECTED_NAME.to_str().unwrap(), Err(IoError::Io));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        assert_eq!(avb_ops.write_persistent_value(EXPECTED_NAME, EXPECTED_VALUE), Err(IoError::Io));
    }

    #[test]
    fn write_persistent_value_not_implemented() {
        const EXPECTED_NAME: &CStr = c"test";
        const EXPECTED_VALUE: &[u8] = b"test";

        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops
            .avb_ops
            .add_persistent_value(EXPECTED_NAME.to_str().unwrap(), Err(IoError::NotImplemented));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        // Dev should report no such value, prod should fail.
        assert_eq!(
            avb_ops.write_persistent_value(EXPECTED_NAME, EXPECTED_VALUE),
            dev_only(Err(IoError::NoSuchValue))
        );
    }

    #[test]
    fn erase_persistent_value_success() {
        const EXPECTED_NAME: &CStr = c"test";

        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_ops.add_persistent_value(EXPECTED_NAME.to_str().unwrap(), Ok(b"test"));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        assert_eq!(avb_ops.erase_persistent_value(EXPECTED_NAME), Ok(()));

        assert!(!gbl_ops.avb_ops.persistent_values.contains_key(EXPECTED_NAME.to_str().unwrap()));
    }

    #[test]
    fn erase_persistent_value_error() {
        const EXPECTED_NAME: &CStr = c"test";

        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops.avb_ops.add_persistent_value(EXPECTED_NAME.to_str().unwrap(), Err(IoError::Io));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        assert_eq!(avb_ops.erase_persistent_value(EXPECTED_NAME), Err(IoError::Io));
    }

    #[test]
    fn erase_persistent_value_not_implemented() {
        const EXPECTED_NAME: &CStr = c"test";

        let mut gbl_ops = FakeGblOps::new(&[]);
        gbl_ops
            .avb_ops
            .add_persistent_value(EXPECTED_NAME.to_str().unwrap(), Err(IoError::NotImplemented));

        let mut avb_ops = GblAvbOps::new(&mut gbl_ops, None, &[], false);
        // Dev should report no such value, prod should fail.
        assert_eq!(
            avb_ops.erase_persistent_value(EXPECTED_NAME),
            dev_only(Err(IoError::NoSuchValue))
        );
    }
}
