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

//! Contains APIs for performaing android verified boot.

use crate::{
    gbl_avb::{
        ops::{GblAvbOps, PreloadBufferState, AVB_DIGEST_KEY},
        state::{BootStateColor, KeyValidationStatus, VerificationStatus},
    },
    gbl_println,
    ops::{PartitionBuffer, Slot},
    slots::Bootability,
    GblOps, Result,
};
use abr::SlotIndex;
use arrayvec::ArrayVec;
use avb::{
    slot_verify, HashtreeErrorMode, SlotVerifyData, SlotVerifyError, SlotVerifyFlags,
    SlotVerifyResult,
};
use bootparams::entry::CommandlineParser;
use core::{ffi::CStr, ops::DerefMut};
use liberror::Error;

// Maximum number of partition allowed for verification.
//
// The value is randomly chosen for now. We can update it as we see more usecases.
pub(crate) const MAX_NUM_PARTITION: usize = 16;

// Type alias for ArrayVec of size `MAX_NUM_PARTITION`:
pub(crate) type ArrayMaxParts<T> = ArrayVec<T, MAX_NUM_PARTITION>;

/// A container holding partitions for libavb verification
pub struct PartitionsToVerify<'a> {
    partitions: ArrayMaxParts<&'a CStr>,
    preloaded: ArrayMaxParts<(&'a str, PreloadBufferState<'a>)>,
}

impl<'a> PartitionsToVerify<'a> {
    /// Appends a partition to verify
    pub fn try_push(&mut self, name: &'a CStr) -> Result<()> {
        self.partitions.try_push(name).or(Err(Error::TooManyPartitions(MAX_NUM_PARTITION)))?;
        Ok(())
    }

    /// Appends a preloaded partition buffer.
    pub fn try_push_preloaded(
        &mut self,
        name: &'a CStr,
        buf: &'a mut PartitionBuffer<impl DerefMut<Target = [u8]>>,
    ) -> Result<()> {
        let buf = match buf {
            PartitionBuffer::Preloaded(ref v) => PreloadBufferState::Loaded(&v[..]),
            PartitionBuffer::Designated(ref mut v) => PreloadBufferState::ToLoad(&mut v[..]),
        };
        let err = Err(Error::TooManyPartitions(MAX_NUM_PARTITION));
        self.partitions.try_push(name).or(err)?;
        self.preloaded.try_push((name.to_str().unwrap(), buf)).or(err)?;
        Ok(())
    }
}

impl<'a> Default for PartitionsToVerify<'a> {
    fn default() -> Self {
        Self { partitions: ArrayMaxParts::new(), preloaded: ArrayMaxParts::new() }
    }
}

/// Consumes a SlotVerifyResult and returns a SlotVerifyData
pub(crate) fn into_verify_data<'a>(
    res: SlotVerifyResult<'a, SlotVerifyData<'a>>,
) -> Option<SlotVerifyData<'a>> {
    match res {
        Ok(data) => Some(data),
        Err(SlotVerifyError::PublicKeyRejected(v)) => v,
        Err(SlotVerifyError::RollbackIndex(v)) => v,
        Err(SlotVerifyError::Verification(v)) => v,
        _ => None,
    }
}

/// Android verified boot flow.
///
/// All relevant images from disk must be preloaded and provided as `partitions`; in its final
/// state `ops` will provide the necessary callbacks for where the images should go in RAM and
/// which ones are preloaded.
///
/// # Arguments
/// * `ops`: [GblOps] providing device-specific backend.
/// * `slot`: Current slot.
/// * `partitions`: [PartitionsToVerify] providing pre-loaded partitions.
///
/// # Returns
///
/// * On success, returns a tuple of (verification result, BootStateColor, is_unlocked).
/// * Returns an error if verification process failed and boot cannot continue.
pub fn avb_verify_slot<'a, 'b, 'c: 'd, 'd>(
    ops: &mut impl GblOps<'a, 'b>,
    slot: Slot,
    partitions: &'d mut PartitionsToVerify<'c>,
) -> Result<(SlotVerifyData<'d>, VerificationStatus, bool)> {
    let slot_index = SlotIndex::try_from(slot.suffix.as_char())
        .inspect_err(|_| gbl_println!(ops, "AVB: Invalid slot: {}", slot.suffix.as_char()))
        .map_err(|_| Error::InvalidInput)?;

    let PartitionsToVerify { partitions, preloaded } = partitions;

    let mut avb_ops = GblAvbOps::new(ops, Some(slot_index), preloaded, false);
    let status = avb_ops.avb_read_device_status()?;

    let mut flags = SlotVerifyFlags::AVB_SLOT_VERIFY_FLAGS_NONE;
    if status.is_unlocked {
        flags |= SlotVerifyFlags::AVB_SLOT_VERIFY_FLAGS_ALLOW_VERIFICATION_ERROR;
    }
    if status.is_dm_verity_error {
        flags |= SlotVerifyFlags::AVB_SLOT_VERIFY_FLAGS_RESTART_CAUSED_BY_HASHTREE_CORRUPTION;
    }

    let verify_result = slot_verify(
        &mut avb_ops,
        partitions,
        Some(slot_index.into()),
        flags,
        HashtreeErrorMode::AVB_HASHTREE_ERROR_MODE_MANAGED_RESTART_AND_EIO,
    );
    let (color, verify_data) = match verify_result {
        Ok(ref verify_data) => {
            let color = match status.is_unlocked {
                false
                    if avb_ops.key_validation_status()? == KeyValidationStatus::ValidCustomKey =>
                {
                    BootStateColor::Yellow
                }
                false => BootStateColor::Green,
                true => BootStateColor::Orange,
            };

            gbl_println!(
                avb_ops.gbl_ops,
                "AVB verification passed. Device is unlocked: {}. Color: {color}",
                status.is_unlocked,
            );

            (color, Some(verify_data))
        }
        // Non-fatal error, can continue booting since verify_data is available.
        Err(ref e) if e.verification_data().is_some() && status.is_unlocked => {
            let color = BootStateColor::Orange;

            gbl_println!(
                avb_ops.gbl_ops,
                "AVB verification failed with {e}. Device is unlocked: {} Color: {color}. \
                Continue current boot attempt.",
                status.is_unlocked
            );

            (color, Some(e.verification_data().unwrap()))
        }
        // Fatal error. Cannot boot.
        Err(ref e) => {
            let color = BootStateColor::Red;

            gbl_println!(
                avb_ops.gbl_ops,
                "AVB verification failed with {e}. Device is unlocked: {}. Color: {color}. \
                Cannot continue boot.",
                status.is_unlocked
            );

            (color, None)
        }
    };

    let mut digest = None;
    let mut is_eio = false;
    if let Some(ref verify_data) = verify_data {
        assert!(
            verify_data.vbmeta_data().first().unwrap().partition_name() == c"vbmeta",
            "GBL requires the vbmeta partition as the top-level verification structure. Please \
            contact the GBL team if you encounter this error."
        );

        is_eio = verify_data.resolved_hashtree_error_mode()
            == HashtreeErrorMode::AVB_HASHTREE_ERROR_MODE_EIO;

        digest = CommandlineParser::new(verify_data.cmdline().to_str().unwrap())
            .find_map(|v| v.ok().filter(|v| v.key == AVB_DIGEST_KEY))
            .map(|v| v.value)
            .flatten()
    }

    let verification_status = VerificationStatus { color, is_eio };
    // Allowes FW to handle verification result.
    avb_ops.handle_verification_result(verify_data, verification_status, digest)?;

    if let Some(ref verify_data) = verify_data {
        // Update rollback indices if the slot has successfully booted following:
        // https://android.googlesource.com/platform/external/avb/+/android16-release/README.md#updating-stored-rollback-indexes
        if !status.is_unlocked && slot.bootability == Bootability::Successful {
            avb_ops.update_rollback_indexes(verify_data)?;
        }
    }

    match color {
        BootStateColor::Red => Err(verify_result.unwrap_err().without_verify_data().into()),
        _ => {
            Ok((into_verify_data(verify_result).unwrap(), verification_status, status.is_unlocked))
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        android_boot::tests::{read_test_data, TEST_ROLLBACK_INDEX, TEST_ROLLBACK_INDEX_LOCATION},
        ops::{
            test::{slot, slot_successful, FakeGblOps, FakeGblOpsStorage},
            AvbDeviceStatus, AvbProperty, Slot,
        },
        IntegrationError::AvbIoError,
    };
    use avb::{IoError, SlotVerifyError};
    use std::{collections::HashMap, ffi::CStr};

    /// FW rollback index before verification.
    const TEST_ROLLBACK_INDEX_BEFORE_VERIFY: u64 = TEST_ROLLBACK_INDEX - 1;

    /// Helper for testing avb_verify_slot
    fn test_avb_verify_slot<'a>(
        partitions: &[(&CStr, &str)],
        partitions_to_verify: &mut PartitionsToVerify<'a>,
        device_status: std::result::Result<AvbDeviceStatus, IoError>,
        key_validation_status: KeyValidationStatus,
        fw_rollback_result: std::result::Result<u64, IoError>,
        slot: Slot,
        expected_updated_fw_rollback: Option<u64>,
        expected_reported_status: Option<VerificationStatus>,
    ) -> Result<()> {
        let mut storage = FakeGblOpsStorage::default();
        for (part, file) in partitions {
            storage.add_raw_device(part, read_test_data(file));
        }
        let mut ops = FakeGblOps::new(&storage);
        match device_status {
            Ok(ref device_status) => ops.avb_device_status = device_status.clone(),
            Err(ref e) => ops.avb_device_status_error = Some(e.clone()),
        };
        ops.avb_ops.rollbacks = HashMap::from([(TEST_ROLLBACK_INDEX_LOCATION, fw_rollback_result)]);
        let mut out_status = None;
        let mut handler = |status, _: Option<&CStr>, _: Option<Vec<AvbProperty<'_>>>| {
            out_status = Some(status);
            Ok(())
        };
        ops.avb_handle_verification_result = Some(&mut handler);
        ops.avb_key_validation_status = Some(Ok(key_validation_status));
        let res = avb_verify_slot(&mut ops, slot, partitions_to_verify);
        if let Some(expected_updated_fw_rollback) = expected_updated_fw_rollback {
            let updated_rollback_index =
                ops.avb_ops.rollbacks.get(&TEST_ROLLBACK_INDEX_LOCATION).unwrap();
            assert_eq!(updated_rollback_index.as_ref().unwrap(), &expected_updated_fw_rollback);
        }
        assert_eq!(out_status, expected_reported_status);
        let (_, status, unlocked) = res?;
        assert_eq!(out_status.unwrap(), status);
        if let Ok(device_status) = device_status {
            assert_eq!(unlocked, device_status.is_unlocked);
        }
        Ok(())
    }

    #[test]
    fn test_avb_verify_slot_success() {
        let mut partitions_to_verify = PartitionsToVerify::default();
        partitions_to_verify.try_push(c"boot").unwrap();
        partitions_to_verify.try_push(c"init_boot").unwrap();
        partitions_to_verify.try_push(c"vendor_boot").unwrap();
        let partitions_data = [
            (c"boot_a", "boot_no_ramdisk_v4_a.img"),
            (c"init_boot_a", "init_boot_a.img"),
            (c"vendor_boot_a", "vendor_boot_v4_a.img"),
            (c"vbmeta_a", "vbmeta_v4_v4_init_boot_a.img"),
        ];

        assert_eq!(
            test_avb_verify_slot(
                &partitions_data,
                &mut partitions_to_verify,
                Ok(AvbDeviceStatus { is_unlocked: false, is_dm_verity_error: false }),
                KeyValidationStatus::Valid,
                // FW rollback index result.
                Ok(TEST_ROLLBACK_INDEX_BEFORE_VERIFY),
                // Slot has been already successfully booted before.
                slot_successful('a'),
                // Rollback index is expected to be updated.
                Some(TEST_ROLLBACK_INDEX),
                Some(VerificationStatus { color: BootStateColor::Green, is_eio: false }),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_avb_verify_slot_success_custom_key() {
        let mut partitions_to_verify = PartitionsToVerify::default();
        partitions_to_verify.try_push(c"boot").unwrap();
        partitions_to_verify.try_push(c"init_boot").unwrap();
        partitions_to_verify.try_push(c"vendor_boot").unwrap();
        let partitions_data = [
            (c"boot_a", "boot_no_ramdisk_v4_a.img"),
            (c"init_boot_a", "init_boot_a.img"),
            (c"vendor_boot_a", "vendor_boot_v4_a.img"),
            (c"vbmeta_a", "vbmeta_v4_v4_init_boot_a.img"),
        ];

        assert_eq!(
            test_avb_verify_slot(
                &partitions_data,
                &mut partitions_to_verify,
                Ok(AvbDeviceStatus { is_unlocked: false, is_dm_verity_error: false }),
                KeyValidationStatus::ValidCustomKey,
                // FW rollback index result.
                Ok(TEST_ROLLBACK_INDEX_BEFORE_VERIFY),
                // Slot has been already successfully booted before.
                slot_successful('a'),
                // Rollback index is expected to be updated.
                Some(TEST_ROLLBACK_INDEX),
                Some(VerificationStatus { color: BootStateColor::Yellow, is_eio: false }),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_avb_verify_slot_from_preloaded_success() {
        let mut boot = read_test_data("boot_no_ramdisk_v4_a.img");
        let mut init_boot = read_test_data("init_boot_a.img");
        let mut vendor_boot = read_test_data("vendor_boot_v4_a.img");

        let mut preloaded = [
            (c"boot", PartitionBuffer::Preloaded(&mut boot[..])),
            (c"init_boot", PartitionBuffer::Preloaded(&mut init_boot[..])),
            (c"vendor_boot", PartitionBuffer::Preloaded(&mut vendor_boot[..])),
        ];
        let mut partitions_to_verify = PartitionsToVerify::default();
        for (n, v) in &mut preloaded {
            partitions_to_verify.try_push_preloaded(n, v).unwrap();
        }
        let partitions_data = [
            // Required images aren't presented. Have to rely on preloaded.
            (c"vbmeta_a", "vbmeta_v4_v4_init_boot_a.img"),
        ];
        assert_eq!(
            test_avb_verify_slot(
                &partitions_data,
                &mut partitions_to_verify,
                Ok(AvbDeviceStatus { is_unlocked: false, is_dm_verity_error: false }),
                KeyValidationStatus::Valid,
                // FW rollback index result.
                Ok(TEST_ROLLBACK_INDEX_BEFORE_VERIFY),
                slot('a'),
                // First boot for slot. Index hasn't been updated.
                Some(TEST_ROLLBACK_INDEX_BEFORE_VERIFY),
                Some(VerificationStatus { color: BootStateColor::Green, is_eio: false }),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_avb_verify_slot_success_unlocked() {
        let mut partitions_to_verify = PartitionsToVerify::default();
        partitions_to_verify.try_push(c"boot").unwrap();
        partitions_to_verify.try_push(c"init_boot").unwrap();
        partitions_to_verify.try_push(c"vendor_boot").unwrap();
        let partitions_data = [
            (c"boot_a", "boot_no_ramdisk_v4_a.img"),
            (c"init_boot_a", "init_boot_a.img"),
            (c"vendor_boot_a", "vendor_boot_v4_a.img"),
            (c"vbmeta_a", "vbmeta_v4_v4_init_boot_a.img"),
        ];
        assert_eq!(
            test_avb_verify_slot(
                &partitions_data,
                &mut partitions_to_verify,
                Ok(AvbDeviceStatus { is_unlocked: true, is_dm_verity_error: false }),
                KeyValidationStatus::Valid,
                // FW rollback index result.
                Ok(TEST_ROLLBACK_INDEX_BEFORE_VERIFY),
                slot_successful('a'),
                // Device is unlocked. Index hasn't been updated.
                Some(TEST_ROLLBACK_INDEX_BEFORE_VERIFY),
                Some(VerificationStatus { color: BootStateColor::Orange, is_eio: false }),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_avb_verify_slot_verification_failed_unlocked() {
        let mut partitions_to_verify = PartitionsToVerify::default();
        partitions_to_verify.try_push(c"boot").unwrap();
        partitions_to_verify.try_push(c"init_boot").unwrap();
        partitions_to_verify.try_push(c"vendor_boot").unwrap();
        let partitions_data = [
            // Wrong boot image, expect verification to fail.
            (c"boot_a", "boot_v0_a.img"),
            (c"init_boot_a", "init_boot_a.img"),
            (c"vendor_boot_a", "vendor_boot_v4_a.img"),
            (c"vbmeta_a", "vbmeta_v4_v4_init_boot_a.img"),
        ];
        assert_eq!(
            test_avb_verify_slot(
                &partitions_data,
                &mut partitions_to_verify,
                Ok(AvbDeviceStatus { is_unlocked: true, is_dm_verity_error: false }),
                KeyValidationStatus::Valid,
                // FW rollback index result.
                Ok(TEST_ROLLBACK_INDEX_BEFORE_VERIFY),
                slot_successful('a'),
                // Device is unlocked. Index hasn't been updated.
                Some(TEST_ROLLBACK_INDEX_BEFORE_VERIFY),
                Some(VerificationStatus { color: BootStateColor::Orange, is_eio: false }),
            ),
            // Device is unlocked, so can continue boot.
            Ok(()),
        );
    }

    #[test]
    fn test_avb_verify_slot_verification_fatal_failed_unlocked() {
        let mut partitions_to_verify = PartitionsToVerify::default();
        partitions_to_verify.try_push(c"boot").unwrap();
        partitions_to_verify.try_push(c"init_boot").unwrap();
        partitions_to_verify.try_push(c"vendor_boot").unwrap();
        let partitions_data = [
            (c"boot_a", "boot_no_ramdisk_v4_a.img"),
            (c"init_boot_a", "init_boot_a.img"),
            (c"vendor_boot_a", "vendor_boot_v4_a.img"),
            (c"vbmeta_a", "vbmeta_v4_v4_init_boot_a.img"),
        ];
        assert_eq!(
            test_avb_verify_slot(
                &partitions_data,
                &mut partitions_to_verify,
                Ok(AvbDeviceStatus { is_unlocked: true, is_dm_verity_error: false }),
                KeyValidationStatus::Valid,
                // FW rollback index result.
                Err(IoError::NoSuchValue),
                slot_successful('a'),
                // Getting FW rollback index is failed. Index cannot be updated and checked.
                None,
                Some(VerificationStatus { color: BootStateColor::Red, is_eio: false }),
            ),
            Err(SlotVerifyError::Io.into())
        )
    }

    #[test]
    fn test_avb_verify_slot_verification_failed_locked() {
        let mut partitions_to_verify = PartitionsToVerify::default();
        partitions_to_verify.try_push(c"boot").unwrap();
        partitions_to_verify.try_push(c"init_boot").unwrap();
        partitions_to_verify.try_push(c"vendor_boot").unwrap();
        let partitions_data = [
            // Wrong boot image, expect verification to fail.
            (c"boot_a", "boot_v0_a.img"),
            (c"init_boot_a", "init_boot_a.img"),
            (c"vendor_boot_a", "vendor_boot_v4_a.img"),
            (c"vbmeta_a", "vbmeta_v4_v4_init_boot_a.img"),
        ];

        assert_eq!(
            test_avb_verify_slot(
                &partitions_data,
                &mut partitions_to_verify,
                Ok(AvbDeviceStatus { is_unlocked: false, is_dm_verity_error: false }),
                KeyValidationStatus::Valid,
                // FW rollback index result.
                Ok(TEST_ROLLBACK_INDEX_BEFORE_VERIFY),
                slot_successful('a'),
                // Verification failed. Index hasn't been updated.
                Some(TEST_ROLLBACK_INDEX_BEFORE_VERIFY),
                Some(VerificationStatus { color: BootStateColor::Red, is_eio: false }),
            ),
            // Cannot continue boot.
            Err(SlotVerifyError::Verification(None).into()),
        );
    }

    #[test]
    fn test_avb_verify_slot_success_eio_mode() {
        let mut partitions_to_verify = PartitionsToVerify::default();
        partitions_to_verify.try_push(c"boot").unwrap();
        partitions_to_verify.try_push(c"init_boot").unwrap();
        partitions_to_verify.try_push(c"vendor_boot").unwrap();
        let partitions_data = [
            (c"boot_a", "boot_no_ramdisk_v4_a.img"),
            (c"init_boot_a", "init_boot_a.img"),
            (c"vendor_boot_a", "vendor_boot_v4_a.img"),
            (c"vbmeta_a", "vbmeta_v4_v4_init_boot_a.img"),
        ];

        assert_eq!(
            test_avb_verify_slot(
                &partitions_data,
                &mut partitions_to_verify,
                Ok(AvbDeviceStatus { is_unlocked: false, is_dm_verity_error: true }),
                KeyValidationStatus::Valid,
                // FW rollback index result.
                Ok(TEST_ROLLBACK_INDEX_BEFORE_VERIFY),
                slot_successful('a'),
                // Rollback index is expected to be updated.
                Some(TEST_ROLLBACK_INDEX),
                // Expected verification status.
                Some(VerificationStatus { color: BootStateColor::Green, is_eio: true }),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_avb_verify_slot_verification_failed_obtain_device_status() {
        let mut partitions_to_verify = PartitionsToVerify::default();

        assert_eq!(
            test_avb_verify_slot(
                &[],
                &mut partitions_to_verify,
                // Device status
                Err(IoError::NoSuchValue),
                KeyValidationStatus::Valid,
                // FW rollback index result.
                Ok(TEST_ROLLBACK_INDEX_BEFORE_VERIFY),
                slot_successful('a'),
                // Verification isn't started. Index hasn't been updated.
                Some(TEST_ROLLBACK_INDEX_BEFORE_VERIFY),
                // Expected verification status.
                None,
            ),
            // Cannot continue boot.
            Err(AvbIoError(IoError::NoSuchValue)),
        );
    }

    #[cfg(feature = "gbl_dev")]
    #[test]
    fn test_avb_verify_slot_avb_not_implemented_dev_gbl() {
        let mut partitions_to_verify = PartitionsToVerify::default();
        partitions_to_verify.try_push(c"boot").unwrap();
        partitions_to_verify.try_push(c"init_boot").unwrap();
        partitions_to_verify.try_push(c"vendor_boot").unwrap();
        let partitions_data = [
            (c"boot_a", "boot_no_ramdisk_v4_a.img"),
            (c"init_boot_a", "init_boot_a.img"),
            (c"vendor_boot_a", "vendor_boot_v4_a.img"),
            (c"vbmeta_a", "vbmeta_v4_v4_init_boot_a.img"),
        ];
        assert_eq!(
            test_avb_verify_slot(
                &partitions_data,
                &mut partitions_to_verify,
                // Get device status isn't implemented.
                Err(IoError::NotImplemented),
                KeyValidationStatus::Valid,
                // Read FW rollback index isn't implemented.
                Err(IoError::NotImplemented),
                slot_successful('a'),
                // Getting FW rollback index is failed. Index cannot be updated and checked.
                None,
                Some(VerificationStatus { color: BootStateColor::Orange, is_eio: false }),
            ),
            Ok(()),
        );
    }
}
