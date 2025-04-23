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

use crate::{
    fuchsia_boot::{zbi_split_unused_buffer_mut, zircon_part_name, SlotIndex},
    gbl_avb::ops::GblAvbOps,
    gbl_println, GblOps, Result as GblResult,
};
use avb::{slot_verify, Descriptor, HashtreeErrorMode, Ops as _, SlotVerifyError, SlotVerifyFlags};
use zbi::{merge_within, ZbiContainer};
use zerocopy::SplitByteSliceMut;

/// Verifies a loaded ZBI kernel.
///
/// # Arguments
///
/// * glb_ops - GblOps implementation
/// * slot - slot to verify
/// * slot_booted_successfully - if true, roll back indexes will be increased
/// * zbi_kernel - preloaded kernel to verify
/// * zbi_items - vbmeta items will be appended to this ZbiContainer
pub(crate) fn zircon_verify_kernel<'a, 'b, 'c, B: SplitByteSliceMut + PartialEq>(
    gbl_ops: &mut impl GblOps<'b, 'c>,
    slot: Option<SlotIndex>,
    slot_booted_successfully: bool,
    zbi_kernel: &'a mut [u8],
    zbi_items: &mut ZbiContainer<B>,
) -> GblResult<()> {
    // Copy ZBI items after kernel first. Because ordering matters, and new items should override
    // older ones.
    // TODO(b/379778252) It is not as efficient as moving kernel since ZBI items would contain file
    // system and be bigger than kernel.
    copy_items_after_kernel(zbi_kernel, zbi_items)?;
    let (kernel, _) = zbi_split_unused_buffer_mut(&mut zbi_kernel[..])?;
    zircon_verify_kernel_internal(gbl_ops, slot, slot_booted_successfully, kernel, zbi_items)
}

/// Copy ZBI items following kernel to separate container.
pub fn copy_items_after_kernel<'a, B: SplitByteSliceMut + PartialEq>(
    zbi_kernel: &'a mut [u8],
    zbi_items: &mut ZbiContainer<B>,
) -> GblResult<()> {
    let zbi_container = ZbiContainer::parse(&mut zbi_kernel[..])?;
    let mut items_iter = zbi_container.iter();
    items_iter.next(); // Skip first kernel item
    zbi_items.extend_items(items_iter)?;
    Ok(())
}

/// Performs AVB verification of a ZBI kernel from the given buffer and fixes up AVB ZBI items into
/// the same ZBI container. `load_buffer` should reserve extra space for in-coming AVB items.
pub(crate) fn zircon_verify_kernel_in_place<'a, 'b>(
    gbl_ops: &mut impl GblOps<'a, 'b>,
    slot: Option<SlotIndex>,
    slot_booted_successfully: bool,
    load_buffer: &mut [u8],
) -> GblResult<()> {
    let (kernel, desc_buf) = zbi_split_unused_buffer_mut(&mut load_buffer[..])?;
    let desc_zbi_off = kernel.len();
    // Collects ZBI items from vbmetadata and appends to the `desc_buf` buffer.
    let mut avb_desc = ZbiContainer::new(&mut desc_buf[..])?;
    zircon_verify_kernel_internal(gbl_ops, slot, slot_booted_successfully, kernel, &mut avb_desc)?;
    // Merges the vbmeta descriptor ZBI container into the ZBI kernel container.
    merge_within(load_buffer, desc_zbi_off)?;
    Ok(())
}

/// Internal helper for AVB verification for zircon.
fn zircon_verify_kernel_internal<'a, 'b, 'c, B: SplitByteSliceMut + PartialEq>(
    gbl_ops: &mut impl GblOps<'b, 'c>,
    slot: Option<SlotIndex>,
    slot_booted_successfully: bool,
    zbi_kernel: &'a mut [u8],
    zbi_items: &mut ZbiContainer<B>,
) -> GblResult<()> {
    let (kernel, _) = zbi_split_unused_buffer_mut(&mut zbi_kernel[..])?;

    // Verifies the kernel.
    let part = zircon_part_name(slot);
    let slotless_part = zircon_part_name(None);
    let preloaded = [(slotless_part, &kernel[..])];
    let mut avb_ops = GblAvbOps::new(gbl_ops, slot, &preloaded[..], true);

    // Determines verify flags and error mode.
    let unlocked = avb_ops.read_is_device_unlocked()?;
    let mode = HashtreeErrorMode::AVB_HASHTREE_ERROR_MODE_EIO; // Don't care for fuchsia
    let flag = match unlocked {
        true => SlotVerifyFlags::AVB_SLOT_VERIFY_FLAGS_ALLOW_VERIFICATION_ERROR,
        _ => SlotVerifyFlags::AVB_SLOT_VERIFY_FLAGS_NONE,
    };

    // TODO(b/334962583): Supports optional additional partitions to verify.
    let verify_res = slot_verify(&mut avb_ops, &[c"zircon"], slot.map(|s| s.into()), flag, mode);
    let verified_success = verify_res.is_ok();
    let verify_data = match verify_res {
        Ok(ref v) => {
            gbl_println!(avb_ops.gbl_ops, "{} successfully verified", part);
            v
        }
        Err(ref e) if e.verification_data().is_some() && unlocked => {
            // Verification failed but was able to load the images from disk,
            // and we're unlocked so it's OK to proceed.
            gbl_println!(
                avb_ops.gbl_ops,
                "Verification failed, but device is unlocked - continuing boot"
            );
            e.verification_data().unwrap()
        }
        Err(SlotVerifyError::InvalidMetadata) | Err(SlotVerifyError::UnsupportedVersion)
            if unlocked =>
        {
            // The vbmetadata is invalid or unknown version, but we're unlocked
            // so we can just return success. In this case there will be no
            // vbmeta items to collect or rollbacks to set.
            //
            // This is useful so that boards don't have to flash a random vbmeta
            // image just to boot.
            gbl_println!(
                avb_ops.gbl_ops,
                "Failed to load vbmeta, but device is unlocked - continuing boot"
            );
            return Ok(());
        }
        Err(e) => {
            // Anything else is a critical failure (e.g. I/O error, OOM, etc).
            // In these cases we want to fail even on an unlocked board because
            // the state is now unknown and unpredictable. It's also more useful
            // for board bringup if we fail when the callbacks are not properly
            // implemented.
            gbl_println!(avb_ops.gbl_ops, "Verification failed {:?}", e);
            return Err(e.without_verify_data().into());
        }
    };

    // Collects ZBI items from vbmetadata and appends to the `zbi_items`.
    for vbmeta_data in verify_data.vbmeta_data() {
        for prop in vbmeta_data.descriptors()?.iter().filter_map(|d| match d {
            Descriptor::Property(p) if p.key.starts_with("zbi") => Some(p),
            _ => None,
        }) {
            zbi_items.extend_unaligned(prop.value)?;
        }
    }

    // Increases rollback indices if the slot has successfully booted.
    if verified_success && slot_booted_successfully && !unlocked {
        for (loc, val) in verify_data.rollback_indexes().iter().enumerate() {
            if *val > 0 && avb_ops.read_rollback_index(loc)? != *val {
                avb_ops.write_rollback_index(loc, *val)?;
            }
        }

        // Increases rollback index values for Fuchsia key version locations.
        for key_version in avb_ops.key_versions {
            match key_version {
                Some((loc, rollback)) if avb_ops.read_rollback_index(loc)? != rollback => {
                    avb_ops.write_rollback_index(loc, rollback)?;
                }
                _ => {}
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        fuchsia_boot::{
            test::{
                append_cmd_line, corrupt_data, create_gbl_ops, create_storage, normalize_zbi,
                read_test_data, TEST_ROLLBACK_INDEX_LOCATION, ZIRCON_A_ZBI_FILE,
            },
            ZIRCON_KERNEL_ALIGN,
        },
        tests::AlignedBuffer,
    };
    use avb::{IoError, CERT_PIK_VERSION_LOCATION, CERT_PSK_VERSION_LOCATION};
    use zbi::ZBI_ALIGNMENT_USIZE;

    // The cert test keys were both generated with rollback version 42.
    const TEST_CERT_PIK_VERSION: u64 = 42;
    const TEST_CERT_PSK_VERSION: u64 = 42;

    /// Creates the buffers used for `zircon_verify_kernel()`.
    ///
    /// # Arguments
    /// * `zbi_file`: name of the ZBI file to preload.
    ///
    /// # Returns
    /// A tuple of containing:
    /// * the load buffer, preloaded with `zbi_file` contents
    /// * the ZBI items buffer, zeroed out
    fn create_verify_buffers(zbi_file: &str) -> (AlignedBuffer, AlignedBuffer) {
        let zbi = &read_test_data(zbi_file);
        let mut load_buffer = AlignedBuffer::new(zbi.len(), ZIRCON_KERNEL_ALIGN);
        load_buffer[..zbi.len()].clone_from_slice(zbi);
        let zbi_items_buffer = AlignedBuffer::new(1024, ZBI_ALIGNMENT_USIZE);
        (load_buffer, zbi_items_buffer)
    }

    #[test]
    fn test_verify_success() {
        let storage = create_storage();
        let mut ops = create_gbl_ops(&storage);

        let (mut load_buffer, mut zbi_items_buffer) = create_verify_buffers(ZIRCON_A_ZBI_FILE);
        let expect_rollback = ops.avb_ops.rollbacks.clone();
        assert!(zircon_verify_kernel(
            &mut ops,
            Some(SlotIndex::A),
            false,
            &mut load_buffer,
            &mut ZbiContainer::new(&mut zbi_items_buffer[..]).unwrap()
        )
        .is_ok());

        // Verifies that vbmeta ZBI items are appended. Non-zbi items are ignored.
        let mut expected_zbi_items = AlignedBuffer::new(1024, ZBI_ALIGNMENT_USIZE);
        let _ = ZbiContainer::new(&mut expected_zbi_items[..]).unwrap();
        append_cmd_line(&mut expected_zbi_items, b"vb_prop_0=val\0");
        append_cmd_line(&mut expected_zbi_items, b"vb_prop_1=val\0");
        assert_eq!(normalize_zbi(&zbi_items_buffer), normalize_zbi(&expected_zbi_items));

        // Slot is not successful, rollback index should not be updated.
        assert_eq!(expect_rollback, ops.avb_ops.rollbacks);
    }

    #[test]
    fn test_verify_update_rollback_index_for_successful_slot() {
        let storage = create_storage();
        let mut ops = create_gbl_ops(&storage);
        let (mut load_buffer, mut zbi_items_buffer) = create_verify_buffers(ZIRCON_A_ZBI_FILE);

        assert!(zircon_verify_kernel(
            &mut ops,
            Some(SlotIndex::A),
            true,
            &mut load_buffer,
            &mut ZbiContainer::new(&mut zbi_items_buffer[..]).unwrap()
        )
        .is_ok());

        // Slot is successful, rollback index should be updated.
        // vbmeta_a has rollback index value 2 at location 1.
        assert_eq!(
            ops.avb_ops.rollbacks,
            [
                (1, Ok(2)),
                (usize::try_from(CERT_PSK_VERSION_LOCATION).unwrap(), Ok(TEST_CERT_PSK_VERSION)),
                (usize::try_from(CERT_PIK_VERSION_LOCATION).unwrap(), Ok(TEST_CERT_PIK_VERSION))
            ]
            .into()
        );
    }

    #[test]
    fn test_verify_failed_on_corrupted_image() {
        let storage = create_storage();
        let mut ops = create_gbl_ops(&storage);
        let (mut load_buffer, mut zbi_items_buffer) = create_verify_buffers(ZIRCON_A_ZBI_FILE);

        let expect_rollback = ops.avb_ops.rollbacks.clone();
        // Corrupts a random kernel bytes. Skips pass two ZBI headers.
        load_buffer[64] = !load_buffer[64];
        let expect_load = load_buffer.to_vec();
        assert!(zircon_verify_kernel(
            &mut ops,
            Some(SlotIndex::A),
            true,
            &mut load_buffer,
            &mut ZbiContainer::new(&mut zbi_items_buffer[..]).unwrap()
        )
        .is_err());
        // Failed while device is locked. ZBI items should not be appended.
        assert_eq!(expect_load, &load_buffer[..]);
        // Rollback index should not be updated on verification failure.
        assert_eq!(expect_rollback, ops.avb_ops.rollbacks);
    }

    #[test]
    fn test_verify_failed_on_corrupted_vbmetadata() {
        let storage = create_storage();
        let mut ops = create_gbl_ops(&storage);
        let (mut load_buffer, mut zbi_items_buffer) = create_verify_buffers(ZIRCON_A_ZBI_FILE);

        let expect_rollback = ops.avb_ops.rollbacks.clone();
        let expect_load = load_buffer.to_vec();
        // Corrupts vbmetadata
        corrupt_data(&mut ops, "vbmeta_a");
        assert!(zircon_verify_kernel(
            &mut ops,
            Some(SlotIndex::A),
            true,
            &mut load_buffer,
            &mut ZbiContainer::new(&mut zbi_items_buffer[..]).unwrap()
        )
        .is_err());
        // Failed while device is locked. ZBI items should not be appended.
        assert_eq!(expect_load, &load_buffer[..]);
        // Rollback index should not be updated on verification failure.
        assert_eq!(expect_rollback, ops.avb_ops.rollbacks);
    }

    #[test]
    fn test_verify_failed_on_rollback_protection() {
        let storage = create_storage();
        let mut ops = create_gbl_ops(&storage);
        let (mut load_buffer, mut zbi_items_buffer) = create_verify_buffers(ZIRCON_A_ZBI_FILE);

        let expect_load = load_buffer.to_vec();
        // vbmeta_a has rollback index value 2 at location 1. Setting min rollback value of 3 should
        // cause rollback protection failure.
        ops.avb_ops.rollbacks.insert(1, Ok(3));
        let expect_rollback = ops.avb_ops.rollbacks.clone();
        assert!(zircon_verify_kernel(
            &mut ops,
            Some(SlotIndex::A),
            true,
            &mut load_buffer,
            &mut ZbiContainer::new(&mut zbi_items_buffer[..]).unwrap()
        )
        .is_err());
        // Failed while device is locked. ZBI items should not be appended.
        assert_eq!(expect_load, &load_buffer[..]);
        // Rollback index should not be updated on verification failure.
        assert_eq!(expect_rollback, ops.avb_ops.rollbacks);
    }

    #[test]
    fn test_verify_failure_when_unlocked() {
        let storage = create_storage();
        let mut ops = create_gbl_ops(&storage);
        let (mut load_buffer, mut zbi_items_buffer) = create_verify_buffers(ZIRCON_A_ZBI_FILE);

        ops.avb_ops.unlock_state = Ok(true);
        let expect_rollback = ops.avb_ops.rollbacks.clone();

        // Corrupts a random kernel bytes. Skips pass two ZBI headers.
        load_buffer[64] = !load_buffer[64];
        // Verification should proceeds OK.
        assert!(zircon_verify_kernel(
            &mut ops,
            Some(SlotIndex::A),
            true,
            &mut load_buffer,
            &mut ZbiContainer::new(&mut zbi_items_buffer[..]).unwrap()
        )
        .is_ok());
        // Verifies that vbmeta ZBI items are appended as long as unlocked.
        let mut expected_zbi_items = AlignedBuffer::new(load_buffer.len(), ZBI_ALIGNMENT_USIZE);
        let _ = ZbiContainer::new(&mut expected_zbi_items[..]).unwrap();
        append_cmd_line(&mut expected_zbi_items, b"vb_prop_0=val\0");
        append_cmd_line(&mut expected_zbi_items, b"vb_prop_1=val\0");
        assert_eq!(normalize_zbi(&zbi_items_buffer), normalize_zbi(&expected_zbi_items));
        // Rollback index should not be updated in any failure cases, even when unlocked.
        assert_eq!(expect_rollback, ops.avb_ops.rollbacks);
    }

    #[test]
    fn test_copy_items_after_kernel() {
        let zbi = &read_test_data(ZIRCON_A_ZBI_FILE);
        let mut load_buffer = AlignedBuffer::new(zbi.len() + 1024, ZIRCON_KERNEL_ALIGN);
        load_buffer[..zbi.len()].clone_from_slice(zbi);
        // Add items that will be copied
        append_cmd_line(&mut load_buffer, b"vb_prop_0=val\0");
        append_cmd_line(&mut load_buffer, b"vb_prop_1=val\0");

        // Create ZBI items container that contain 1 element
        let mut zbi_items_buffer = AlignedBuffer::new(1024, ZBI_ALIGNMENT_USIZE);
        let _ = ZbiContainer::new(&mut zbi_items_buffer[..]).unwrap();
        append_cmd_line(&mut zbi_items_buffer, b"vb_prop_2=val\0");
        let mut zbi_items = ZbiContainer::parse(&mut zbi_items_buffer[..]).unwrap();

        // Verifies that ZBI items are appended
        let mut expected_zbi_items = AlignedBuffer::new(load_buffer.len(), ZBI_ALIGNMENT_USIZE);
        let _ = ZbiContainer::new(&mut expected_zbi_items[..]).unwrap();
        append_cmd_line(&mut expected_zbi_items, b"vb_prop_2=val\0");
        append_cmd_line(&mut expected_zbi_items, b"vb_prop_0=val\0");
        append_cmd_line(&mut expected_zbi_items, b"vb_prop_1=val\0");

        copy_items_after_kernel(&mut load_buffer, &mut zbi_items).unwrap();
        assert_eq!(normalize_zbi(&zbi_items_buffer), normalize_zbi(&expected_zbi_items));
    }

    #[test]
    fn test_verify_failure_by_corrupted_vbmetadata_unlocked() {
        let storage = create_storage();
        let mut ops = create_gbl_ops(&storage);
        let (mut load_buffer, mut zbi_items_buffer) = create_verify_buffers(ZIRCON_A_ZBI_FILE);

        ops.avb_ops.unlock_state = Ok(true);
        let expect_rollback = ops.avb_ops.rollbacks.clone();
        let expect_load = load_buffer.to_vec();
        // Corrupts vbmetadata
        corrupt_data(&mut ops, "vbmeta_a");
        assert!(zircon_verify_kernel(
            &mut ops,
            Some(SlotIndex::A),
            true,
            &mut load_buffer,
            &mut ZbiContainer::new(&mut zbi_items_buffer[..]).unwrap()
        )
        .is_ok());
        // Unlocked but vbmetadata is invalid so no ZBI items should be appended.
        assert_eq!(expect_load, &load_buffer[..]);
        // Rollback index should not be updated on verification failure.
        assert_eq!(expect_rollback, ops.avb_ops.rollbacks);
    }

    #[test]
    fn test_verify_failure_by_io_error_unlocked() {
        let storage = create_storage();
        let mut ops = create_gbl_ops(&storage);
        let (mut load_buffer, mut zbi_items_buffer) = create_verify_buffers(ZIRCON_A_ZBI_FILE);

        ops.avb_ops.unlock_state = Ok(true);

        // Make some verification callback return I/O error.
        ops.avb_ops.rollbacks.insert(TEST_ROLLBACK_INDEX_LOCATION, Err(IoError::Io));
        let expect_rollback = ops.avb_ops.rollbacks.clone();

        // Even when unlocked, I/O error represents a critical failure and
        // should refuse to verify.
        assert_eq!(
            zircon_verify_kernel(
                &mut ops,
                Some(SlotIndex::A),
                true,
                &mut load_buffer,
                &mut ZbiContainer::new(&mut zbi_items_buffer[..]).unwrap()
            )
            .unwrap_err(),
            SlotVerifyError::Io.into()
        );
        // Rollback index should not be updated on verification failure.
        assert_eq!(expect_rollback, ops.avb_ops.rollbacks);
    }

    #[test]
    fn test_verify_with_unimplemented_ops() {
        let storage = create_storage();
        let mut ops = create_gbl_ops(&storage);
        let (mut load_buffer, mut zbi_items_buffer) = create_verify_buffers(ZIRCON_A_ZBI_FILE);

        // Set the AVB ops to return `NotImplemented`.
        ops.avb_ops.unlock_state = Err(IoError::NotImplemented);
        ops.avb_cert_read_permanent_attributes_not_implemented = true;
        ops.avb_cert_read_permanent_attributes_hash_not_implemented = true;
        ops.avb_ops.rollbacks.insert(TEST_ROLLBACK_INDEX_LOCATION, Err(IoError::NotImplemented));
        ops.avb_ops.rollbacks.insert(CERT_PIK_VERSION_LOCATION, Err(IoError::NotImplemented));
        ops.avb_ops.rollbacks.insert(CERT_PSK_VERSION_LOCATION, Err(IoError::NotImplemented));

        let expect_rollback = ops.avb_ops.rollbacks.clone();
        let result = zircon_verify_kernel(
            &mut ops,
            Some(SlotIndex::A),
            true,
            &mut load_buffer,
            &mut ZbiContainer::new(&mut zbi_items_buffer[..]).unwrap(),
        );

        if cfg!(feature = "gbl_dev") {
            // On dev builds, unimplemented ops should allow booting by default.
            assert!(result.is_ok());

            // The vbmeta ZBI items should be appended.
            let mut expected_zbi_items = AlignedBuffer::new(load_buffer.len(), ZBI_ALIGNMENT_USIZE);
            let _ = ZbiContainer::new(&mut expected_zbi_items[..]).unwrap();
            append_cmd_line(&mut expected_zbi_items, b"vb_prop_0=val\0");
            append_cmd_line(&mut expected_zbi_items, b"vb_prop_1=val\0");
            assert_eq!(normalize_zbi(&zbi_items_buffer), normalize_zbi(&expected_zbi_items));

            // Unimplemented rollback indices should not attempt to update.
            assert_eq!(expect_rollback, ops.avb_ops.rollbacks);
        } else {
            // On prod builds, unimplemented ops should fail verification.
            assert!(result.is_err());
        }
    }
}
