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
        state::{BootStateColor, KeyValidationStatus},
    },
    gbl_println,
    ops::PartitionBuffer,
    GblOps, Result,
};
use abr::SlotIndex;
use arrayvec::ArrayVec;
use avb::{
    slot_verify, HashtreeErrorMode, Ops as _, SlotVerifyData, SlotVerifyError, SlotVerifyFlags,
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
/// * `slot`: The slot index.
/// * `partitions`: [PartitionsToVerify] providing pre-loaded partitions.
///
/// # Returns
///
/// * On success, returns a tuple of (verification result, BootStateColor, is_unlocked).
/// * Returns an error if verification process failed and boot cannot continue.
pub fn avb_verify_slot<'a, 'b, 'c: 'd, 'd>(
    ops: &mut impl GblOps<'a, 'b>,
    slot: u8,
    partitions: &'d mut PartitionsToVerify<'c>,
) -> Result<(SlotVerifyData<'d>, BootStateColor, bool)> {
    let slot = match slot {
        0 => SlotIndex::A,
        1 => SlotIndex::B,
        _ => {
            gbl_println!(ops, "AVB: Invalid slot index: {slot}");
            return Err(Error::InvalidInput.into());
        }
    };

    let PartitionsToVerify { partitions, preloaded } = partitions;

    let mut avb_ops = GblAvbOps::new(ops, Some(slot), preloaded, false);
    let unlocked = avb_ops.read_is_device_unlocked()?;
    let verify_result = slot_verify(
        &mut avb_ops,
        partitions,
        Some(slot.into()),
        // TODO(b/337846185): Pass AVB_SLOT_VERIFY_FLAGS_RESTART_CAUSED_BY_HASHTREE_CORRUPTION in
        // case verity corruption is detected by HLOS.
        match unlocked {
            true => SlotVerifyFlags::AVB_SLOT_VERIFY_FLAGS_ALLOW_VERIFICATION_ERROR,
            _ => SlotVerifyFlags::AVB_SLOT_VERIFY_FLAGS_NONE,
        },
        // TODO(b/337846185): For demo, we use the same setting as Cuttlefish u-boot.
        // Pass AVB_HASHTREE_ERROR_MODE_MANAGED_RESTART_AND_EIO and handle EIO.
        HashtreeErrorMode::AVB_HASHTREE_ERROR_MODE_RESTART_AND_INVALIDATE,
    );

    let (color, verify_data) = match verify_result {
        Ok(ref verify_data) => {
            let color = match unlocked {
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
                "AVB verification passed. Device is unlocked: {unlocked}. Color: {color}"
            );

            (color, Some(verify_data))
        }
        // Non-fatal error, can continue booting since verify_data is available.
        Err(ref e) if e.verification_data().is_some() && unlocked => {
            let color = BootStateColor::Orange;

            gbl_println!(
                avb_ops.gbl_ops,
                "AVB verification failed with {e}. Device is unlocked: {unlocked}. Color: {color}. \
                Continue current boot attempt."
            );

            (color, Some(e.verification_data().unwrap()))
        }
        // Fatal error. Cannot boot.
        Err(ref e) => {
            let color = BootStateColor::Red;

            gbl_println!(
                avb_ops.gbl_ops,
                "AVB verification failed with {e}. Device is unlocked: {unlocked}. Color: {color}. \
                Cannot continue boot."
            );

            (color, None)
        }
    };

    // Gets digest from the result command line.
    let mut digest = None;
    if let Some(ref verify_data) = verify_data {
        digest = CommandlineParser::new(verify_data.cmdline().to_str().unwrap())
            .find_map(|v| v.ok().filter(|v| v.key == AVB_DIGEST_KEY))
            .map(|v| v.value)
            .flatten()
    }
    // Allowes FW to handle verification result.
    avb_ops.handle_verification_result(verify_data, color, digest)?;

    match color {
        BootStateColor::Red => Err(verify_result.unwrap_err().without_verify_data().into()),
        _ => Ok((into_verify_data(verify_result).unwrap(), color, unlocked)),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        android_boot::tests::read_test_data,
        ops::{
            test::{FakeGblOps, FakeGblOpsStorage},
            AvbProperty,
        },
        IntegrationError::AvbIoError,
    };
    use avb::{IoError, SlotVerifyError};
    use std::{collections::HashMap, ffi::CStr};

    /// Helper for testing avb_verify_slot
    fn test_avb_verify_slot<'a>(
        partitions: &[(&CStr, &str)],
        partitions_to_verify: &mut PartitionsToVerify<'a>,
        device_unlocked: std::result::Result<bool, avb::IoError>,
        rollback_result: std::result::Result<u64, avb::IoError>,
        slot: u8,
        expected_reported_color: Option<BootStateColor>,
    ) -> Result<()> {
        let mut storage = FakeGblOpsStorage::default();
        for (part, file) in partitions {
            storage.add_raw_device(part, read_test_data(file));
        }
        let mut ops = FakeGblOps::new(&storage);
        match device_unlocked {
            Ok(unlocked) => ops.avb_device_status.is_unlocked = unlocked,
            Err(ref e) => ops.avb_device_status_error = Some(e.clone()),
        };
        ops.avb_ops.rollbacks = HashMap::from([(1, rollback_result)]);
        let mut out_color = None;
        let mut handler = |color, _: Option<&CStr>, _: Option<Vec<AvbProperty<'_>>>| {
            out_color = Some(color);
            Ok(())
        };
        ops.avb_handle_verification_result = Some(&mut handler);
        ops.avb_key_validation_status = Some(Ok(KeyValidationStatus::Valid));
        let res = avb_verify_slot(&mut ops, slot, partitions_to_verify);
        assert_eq!(out_color, expected_reported_color);
        let (_, _, unlocked) = res?;
        assert_eq!(unlocked, device_unlocked.unwrap());
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
                // Unlocked result
                Ok(false),
                // Rollback index result
                Ok(0),
                // Slot
                0,
                // Expected color
                Some(BootStateColor::Green),
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
                // Unlocked result
                Ok(false),
                // Rollback index result
                Ok(0),
                // Slot
                0,
                // Expected color
                Some(BootStateColor::Green),
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
                // Unlocked result
                Ok(true),
                // Rollback index result
                Ok(0),
                // Slot
                0,
                // Expected color
                Some(BootStateColor::Orange),
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
            (c"boot_a", "boot_no_ramdisk_v4_a.img"),
            (c"init_boot_a", "init_boot_a.img"),
            (c"vendor_boot_a", "vendor_boot_v4_a.img"),
            (c"vbmeta_a", "vbmeta_v4_v4_init_boot_a.img"),
        ];
        assert_eq!(
            test_avb_verify_slot(
                &partitions_data,
                &mut partitions_to_verify,
                // Unlocked result
                Ok(true),
                // Rollback index result
                Ok(0),
                // Slot
                0,
                // Expected color
                Some(BootStateColor::Orange),
            ),
            // Device is unlocked, so can continue boot
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
                // Unlocked result
                Ok(true),
                // Get rollback index is failed
                Err(IoError::NoSuchValue),
                // Slot
                0,
                // Expected color
                Some(BootStateColor::Red),
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
                // Unlocked result
                Ok(false),
                // Rollback index result
                Ok(0),
                // Slot
                0,
                // Expected color
                Some(BootStateColor::Red),
            ),
            // Cannot continue boot
            Err(SlotVerifyError::Verification(None).into()),
        );
    }

    #[test]
    fn test_avb_verify_slot_verification_failed_obtain_lock_status() {
        let mut partitions_to_verify = PartitionsToVerify::default();

        assert_eq!(
            test_avb_verify_slot(
                &[],
                &mut partitions_to_verify,
                // Unlocked result
                Err(avb::IoError::NoSuchValue),
                // Rollback index result
                Ok(0),
                // Slot
                0,
                // Expected color
                None,
            ),
            // Cannot continue boot
            Err(AvbIoError(IoError::NoSuchValue)),
        );
    }
}
