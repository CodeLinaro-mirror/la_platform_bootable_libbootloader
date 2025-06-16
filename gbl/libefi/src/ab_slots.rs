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

extern crate gbl_storage;
extern crate libgbl as gbl;

use crate::protocol::{gbl_efi_ab_slot as ab_slot, Protocol};
use core::convert::TryInto;
use efi_types::{
    GBL_EFI_BOOT_MODE_BOOTLOADER as MODE_BOOTLOADER, GBL_EFI_BOOT_MODE_RECOVERY as MODE_RECOVERY,
};
use gbl::slots::{
    BootTarget, BootToken, Manager, OneShot, RecoveryTarget, Slot, SlotIterator, Suffix, Tries,
    UnbootableReason,
};
use liberror::{Error, Result};

/// Implementation for A/B slot manager based on custom EFI protocol.
pub struct ABManager<'a> {
    protocol: Protocol<'a, ab_slot::GblSlotProtocol>,
    boot_token: Option<BootToken>,
    last_set_active_idx: Option<u8>,
}

impl<'a> ABManager<'a> {
    #[cfg(test)]
    fn new_without_token(protocol: Protocol<'a, ab_slot::GblSlotProtocol>) -> Self {
        Self { protocol, boot_token: None, last_set_active_idx: None }
    }
}

impl gbl::slots::private::SlotGet for ABManager<'_> {
    fn get_slot_by_number(&self, number: usize) -> Result<Slot> {
        let idx = u8::try_from(number).or(Err(Error::BadIndex(number)))?;
        let info = self.protocol.get_slot_info(idx).or(Err(Error::BadIndex(number)))?;
        info.try_into()
    }
}

impl Manager for ABManager<'_> {
    fn get_boot_target(&self) -> Result<BootTarget> {
        let slot = self.get_slot_last_set_active()?;
        let mode = self.protocol.get_boot_mode()?;
        let target = match mode {
            MODE_RECOVERY => BootTarget::Recovery(RecoveryTarget::Slotted(slot)),
            _ => BootTarget::NormalBoot(slot),
        };
        Ok(target)
    }

    fn slots_iter(&self) -> SlotIterator {
        SlotIterator::new(self)
    }

    fn get_slot_last_set_active(&self) -> Result<Slot> {
        use gbl::slots::private::SlotGet;

        if let Some(idx) = self.last_set_active_idx {
            self.get_slot_by_number(idx.into())
        } else {
            self.protocol.get_current_slot()?.try_into()
        }
    }

    fn mark_boot_attempt(&mut self) -> Result<BootToken> {
        self.boot_token.take().ok_or(Error::OperationProhibited)
    }

    fn set_active_slot(&mut self, slot_suffix: Suffix) -> Result<()> {
        let idx: u8 = self
            .slots_iter()
            .position(|s| s.suffix == slot_suffix)
            .ok_or(Error::InvalidInput)?
            .try_into()
            // This 'or' is technically unreachable because the protocol
            // can't give us an index larger than a u8.
            .or(Err(Error::Other(None)))?;
        self.protocol.set_active_slot(idx).or(Err(Error::Other(None))).and_then(|_| {
            self.last_set_active_idx = Some(idx);
            Ok(())
        })
    }

    fn set_slot_unbootable(&mut self, slot_suffix: Suffix, reason: UnbootableReason) -> Result<()> {
        let idx: u8 = self
            .slots_iter()
            .position(|s| s.suffix == slot_suffix)
            .ok_or(Error::InvalidInput)?
            .try_into()
            // This 'or' is technically unreachable because the protocol
            // can't give us an index larger than a u8.
            .or(Err(Error::Other(None)))?;
        self.protocol.set_slot_unbootable(idx, u8::from(reason).into())
    }

    fn get_max_retries(&self) -> Result<Tries> {
        Ok(self.protocol.load_boot_data()?.max_retries.into())
    }

    fn get_oneshot_status(&self) -> Option<OneShot> {
        match self.protocol.get_boot_mode() {
            Ok(MODE_BOOTLOADER) => Some(OneShot::Bootloader),
            _ => None,
        }
    }

    fn set_oneshot_status(&mut self, os: OneShot) -> Result<()> {
        // Android doesn't have a concept of OneShot to recovery.
        match os {
            OneShot::Bootloader => {
                self.protocol.set_boot_mode(MODE_BOOTLOADER).or(Err(Error::Other(None)))
            }
            _ => Err(Error::OperationProhibited),
        }
    }

    fn clear_oneshot_status(&mut self) {}

    fn write_back(&mut self, _: &mut dyn FnMut(&mut [u8]) -> Result<()>) {
        // Note: `expect` instead of swallowing the error.
        // It is important that changes are not silently dropped.
        self.protocol.flush().expect("could not write back modifications to slot metadata");
    }
}

#[cfg(test)]
mod test {
    extern crate avb_sysdeps;

    use super::*;
    use crate::protocol::Protocol;
    use crate::test::*;
    use crate::EfiEntry;
    use core::{ops::DerefMut, time::Duration};
    use efi_types::{
        EfiStatus, GblEfiABSlotProtocol, GblEfiSlotInfo, GblEfiSlotMetadataBlock,
        EFI_STATUS_INVALID_PARAMETER, EFI_STATUS_SUCCESS, GBL_EFI_BOOT_MODE_NORMAL as MODE_NORMAL,
    };
    use gbl::{
        ops::{
            AvbIoResult, CertPermanentAttributes, RebootMode, SlotsMetadata, SHA256_DIGEST_SIZE,
        },
        partition::GblDisk,
        slots::{Bootability, Cursor, RecoveryTarget, UnbootableReason},
        Gbl, GblOps, Os, Result as GblResult,
    };
    use gbl_storage::{BlockIo, BlockIoNull, Disk, Gpt};
    use libgbl::{
        device_tree::DeviceTreeComponentsRegistry,
        gbl_avb::state::{BootStateColor, KeyValidationStatus},
        ops::{FailSender, ImageBuffer, InfoSender, OkaySender},
    };
    use libprofile::{ProfileBackend, ProfileTimer, Reporter};
    use std::{
        ffi::CStr,
        fmt::Write,
        num::NonZeroUsize,
        sync::atomic::{AtomicBool, AtomicU32, Ordering},
    };
    use zbi::ZbiContainer;

    // The thread-local atomics are an ugly, ugly hack to pass state between
    // the protocol method functions and the rest of the test body.
    // Because the variables are thread-local, it is safe to run tests concurrently
    // so long as they establish correct initial values.
    // Also, because no atomic is being read or written to by more than one thread,
    // Ordering::Relaxed is perfectly fine.
    thread_local! {
        static ATOMIC: AtomicBool = AtomicBool::new(false);
    }

    thread_local! {
        static BOOT_MODE: AtomicU32 = AtomicU32::new(MODE_NORMAL);
    }

    // This provides reasonable defaults for all tests that need to get slot info.
    //
    // SAFETY: checks that `info` is properly aligned and not null.
    // Caller must make sure `info` points to a valid GblEfiSlotInfo struct.
    unsafe extern "efiapi" fn get_info(
        _: *mut GblEfiABSlotProtocol,
        idx: u8,
        info: *mut GblEfiSlotInfo,
    ) -> EfiStatus {
        if !info.is_null() && info.is_aligned() && idx < 3 {
            let slot_info = GblEfiSlotInfo {
                suffix: ('a' as u8 + idx).into(),
                unbootable_reason: 0,
                priority: idx + 1,
                tries: idx,
                successful: 2 & idx,
            };
            unsafe { *info = slot_info };
            EFI_STATUS_SUCCESS
        } else {
            EFI_STATUS_INVALID_PARAMETER
        }
    }

    extern "efiapi" fn flush(_: *mut GblEfiABSlotProtocol) -> EfiStatus {
        ATOMIC.with(|a| a.store(true, Ordering::Relaxed));
        EFI_STATUS_SUCCESS
    }

    #[derive(Copy, Clone)]
    struct NullProfiler {}

    impl ProfileBackend for NullProfiler {
        fn new_timer(&self) -> impl ProfileTimer {
            *self
        }

        fn reporter(&self) -> impl Reporter {
            *self
        }
    }

    impl ProfileTimer for NullProfiler {
        fn elapsed(&self) -> Duration {
            Duration::ZERO
        }
    }

    impl Reporter for NullProfiler {
        fn report(&self, _: &'static str, _: &'static str, _: Duration) {}
    }

    struct TestGblOps<'a> {
        manager: ABManager<'a>,
    }

    impl<'a> TestGblOps<'a> {
        fn new(protocol: Protocol<'a, ab_slot::GblSlotProtocol>) -> Self {
            Self { manager: ABManager::new_without_token(protocol) }
        }
    }

    impl<'a, 'd> GblOps<'a, 'd> for TestGblOps<'_> {
        fn console_out(&mut self) -> Option<&mut dyn Write> {
            unimplemented!();
        }

        fn should_stop_in_fastboot(&mut self) -> Result<bool> {
            unimplemented!();
        }

        fn reboot(&mut self) {
            unimplemented!();
        }

        fn disks(
            &self,
        ) -> &'a [GblDisk<
            Disk<impl BlockIo + 'a, impl DerefMut<Target = [u8]> + 'a>,
            Gpt<impl DerefMut<Target = [u8]> + 'a>,
        >] {
            &[] as &[GblDisk<Disk<BlockIoNull, &mut [u8]>, Gpt<&mut [u8]>>]
        }

        fn expected_os(&mut self) -> Result<Option<Os>> {
            Ok(None)
        }

        fn zircon_add_device_zbi_items(&mut self, _: &mut ZbiContainer<&mut [u8]>) -> Result<()> {
            unimplemented!();
        }

        fn get_zbi_bootloader_files_buffer(&mut self) -> Option<&mut [u8]> {
            None
        }

        fn load_slot_interface<'b>(
            &'b mut self,
            persist: &'b mut dyn FnMut(&mut [u8]) -> Result<()>,
            boot_token: BootToken,
        ) -> GblResult<Cursor<'b>> {
            self.manager.boot_token = Some(boot_token);
            Ok(Cursor { ctx: &mut self.manager, persist })
        }

        fn avb_read_is_dm_verity_error(&mut self) -> AvbIoResult<bool> {
            unimplemented!();
        }

        fn avb_read_is_device_unlocked(&mut self) -> AvbIoResult<bool> {
            unimplemented!();
        }

        fn avb_read_rollback_index(&mut self, _rollback_index_location: usize) -> AvbIoResult<u64> {
            unimplemented!();
        }

        fn avb_write_rollback_index(
            &mut self,
            _rollback_index_location: usize,
            _index: u64,
        ) -> AvbIoResult<()> {
            unimplemented!();
        }

        fn avb_validate_vbmeta_public_key(
            &self,
            _public_key: &[u8],
            _public_key_metadata: Option<&[u8]>,
        ) -> AvbIoResult<KeyValidationStatus> {
            unimplemented!();
        }

        fn avb_cert_read_permanent_attributes(
            &mut self,
            _attributes: &mut CertPermanentAttributes,
        ) -> AvbIoResult<()> {
            unimplemented!();
        }

        fn avb_cert_read_permanent_attributes_hash(
            &mut self,
        ) -> AvbIoResult<[u8; SHA256_DIGEST_SIZE]> {
            unimplemented!();
        }

        fn avb_read_persistent_value(
            &mut self,
            _name: &CStr,
            _value: &mut [u8],
        ) -> AvbIoResult<usize> {
            unimplemented!();
        }

        fn avb_write_persistent_value(&mut self, _name: &CStr, _value: &[u8]) -> AvbIoResult<()> {
            unimplemented!();
        }

        fn avb_erase_persistent_value(&mut self, _name: &CStr) -> AvbIoResult<()> {
            unimplemented!();
        }

        fn avb_handle_verification_result(
            &mut self,
            _color: BootStateColor,
            _digest: Option<&CStr>,
            _boot_os_version: Option<&[u8]>,
            _boot_security_patch: Option<&[u8]>,
            _system_os_version: Option<&[u8]>,
            _system_security_patch: Option<&[u8]>,
            _vendor_os_version: Option<&[u8]>,
            _vendor_security_patch: Option<&[u8]>,
        ) -> AvbIoResult<()> {
            unimplemented!();
        }

        fn avf_is_supported(&mut self) -> Result<bool> {
            unimplemented!();
        }

        fn avf_read_vendor_dice_handover<'c>(&mut self, _buffer: &'c mut [u8]) -> Result<&'c [u8]> {
            unimplemented!();
        }

        fn avf_read_secretkeeper_public_key<'c>(
            &mut self,
            _buffer: &'c mut [u8],
        ) -> Result<Option<&'c [u8]>> {
            unimplemented!();
        }

        fn get_image_buffer(
            &mut self,
            _image_name: &str,
            _size: NonZeroUsize,
        ) -> GblResult<ImageBuffer<'d>> {
            unimplemented!();
        }

        fn get_custom_device_tree(&mut self) -> Option<&'a [u8]> {
            unimplemented!();
        }

        fn fixup_bootconfig<'c>(
            &mut self,
            _bootconfig: &[u8],
            _fixup_buffer: &'c mut [u8],
        ) -> Result<Option<&'c [u8]>> {
            unimplemented!();
        }

        fn fixup_device_tree(&mut self, _device_tree: &mut [u8]) -> Result<()> {
            unimplemented!();
        }

        fn select_device_trees(
            &mut self,
            _components: &mut DeviceTreeComponentsRegistry,
        ) -> Result<()> {
            unimplemented!();
        }

        fn fastboot_variable<'arg>(
            &mut self,
            _: &CStr,
            _: impl Iterator<Item = &'arg CStr> + Clone,
            _: &mut [u8],
        ) -> Result<usize> {
            unimplemented!()
        }

        fn fastboot_visit_all_variables(&mut self, _: impl FnMut(&[&CStr], &CStr)) -> Result<()> {
            unimplemented!()
        }

        fn fastboot_run_oem(
            &mut self,
            _: &str,
            _: &mut [u8],
            _: impl InfoSender + OkaySender + FailSender,
        ) -> Result<()> {
            unimplemented!()
        }

        fn fastboot_get_staged(&mut self, _: &mut [u8]) -> Result<(usize, usize)> {
            unimplemented!()
        }

        fn slots_metadata(&mut self) -> Result<SlotsMetadata> {
            unimplemented!();
        }

        fn get_current_slot(&mut self) -> Result<Slot> {
            unimplemented!()
        }

        fn get_next_slot(&mut self, _: bool) -> Result<Slot> {
            unimplemented!()
        }

        fn set_active_slot(&mut self, _: u8) -> Result<()> {
            unimplemented!()
        }

        fn set_reboot_mode(&mut self, _: RebootMode) -> Result<()> {
            unimplemented!()
        }

        fn get_reboot_mode(&mut self) -> Result<RebootMode> {
            unimplemented!()
        }

        fn get_base_sp(&mut self) -> Option<usize> {
            None
        }

        fn get_profiling_backend(&self) -> impl ProfileBackend {
            NullProfiler {}
        }
    }

    #[test]
    fn test_manager_flush_on_close() {
        ATOMIC.with(|a| a.store(false, Ordering::Relaxed));
        run_test(|image_handle, systab_ptr| {
            let mut ab = GblEfiABSlotProtocol { flush: Some(flush), ..Default::default() };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol = generate_protocol::<ab_slot::GblSlotProtocol>(&efi_entry, &mut ab);

            {
                let mut persist = |_: &mut [u8]| Ok(());
                let mut test_ops = TestGblOps::new(protocol);
                let mut gbl = Gbl::<TestGblOps>::new(&mut test_ops);
                let _ = gbl.load_slot_interface(&mut persist).unwrap();
            }
        });
        assert!(ATOMIC.with(|a| a.load(Ordering::Relaxed)));
    }

    #[test]
    fn test_iterator() {
        run_test(|image_handle, systab_ptr| {
            let mut ab = GblEfiABSlotProtocol {
                get_slot_info: Some(get_info),
                flush: Some(flush),
                ..Default::default()
            };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol = generate_protocol::<ab_slot::GblSlotProtocol>(&efi_entry, &mut ab);
            let mut persist = |_: &mut [u8]| Ok(());
            let mut test_ops = TestGblOps::new(protocol);
            let mut gbl = Gbl::<TestGblOps>::new(&mut test_ops);
            let cursor = gbl.load_slot_interface(&mut persist).unwrap();

            let slots: Vec<Slot> = cursor.ctx.slots_iter().collect();
            assert_eq!(
                slots,
                vec![
                    Slot {
                        suffix: 'a'.into(),
                        priority: 1usize.into(),
                        bootability: Bootability::Unbootable(UnbootableReason::Unknown),
                    },
                    Slot {
                        suffix: 'b'.into(),
                        priority: 2usize.into(),
                        bootability: Bootability::Retriable(1usize.into()),
                    },
                    Slot {
                        suffix: 'c'.into(),
                        priority: 3usize.into(),
                        bootability: Bootability::Successful,
                    }
                ]
            )
        });
    }

    #[test]
    fn test_active_slot() {
        // SAFETY: verfies that `info` properly aligned and not null.
        // It is the callers responsibility to make sure
        // that `info` points to a valid GblEfiSlotInfo.
        unsafe extern "efiapi" fn get_current_slot(
            _: *mut GblEfiABSlotProtocol,
            info: *mut GblEfiSlotInfo,
        ) -> EfiStatus {
            if info.is_null() || !info.is_aligned() {
                return EFI_STATUS_INVALID_PARAMETER;
            }
            let slot_info = GblEfiSlotInfo {
                suffix: 'a' as u32,
                unbootable_reason: 0,
                priority: 7,
                tries: 15,
                successful: 1,
            };

            unsafe { *info = slot_info };
            EFI_STATUS_SUCCESS
        }

        // SAFETY:
        // `mode` must point to non-null u32 buffer available to write.
        unsafe extern "efiapi" fn get_boot_mode(
            _: *mut GblEfiABSlotProtocol,
            mode: *mut u32,
        ) -> EfiStatus {
            if mode.is_null() || !mode.is_aligned() {
                return EFI_STATUS_INVALID_PARAMETER;
            }

            // SAFETY:
            // `mode` is non null and points to a u32 buffer available to write.
            unsafe {
                *mode = BOOT_MODE.with(|r| r.load(Ordering::Relaxed));
            }
            EFI_STATUS_SUCCESS
        }

        run_test(|image_handle, systab_ptr| {
            let mut ab = GblEfiABSlotProtocol {
                get_current_slot: Some(get_current_slot),
                get_boot_mode: Some(get_boot_mode),
                flush: Some(flush),
                ..Default::default()
            };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol = generate_protocol::<ab_slot::GblSlotProtocol>(&efi_entry, &mut ab);
            let mut persist = |_: &mut [u8]| Ok(());
            let mut test_ops = TestGblOps::new(protocol);
            let mut gbl = Gbl::<TestGblOps>::new(&mut test_ops);
            let cursor = gbl.load_slot_interface(&mut persist).unwrap();

            let slot = Slot {
                suffix: 'a'.into(),
                priority: 7usize.into(),
                bootability: Bootability::Successful,
            };
            assert_eq!(cursor.ctx.get_boot_target().unwrap(), BootTarget::NormalBoot(slot));
            assert_eq!(cursor.ctx.get_slot_last_set_active().unwrap(), slot);

            BOOT_MODE.with(|r| r.store(MODE_RECOVERY, Ordering::Relaxed));

            assert_eq!(
                cursor.ctx.get_boot_target().unwrap(),
                BootTarget::Recovery(RecoveryTarget::Slotted(slot))
            );
        });
    }

    #[test]
    fn test_mark_boot_attempt() {
        run_test(|image_handle, systab_ptr| {
            let mut ab = GblEfiABSlotProtocol { flush: Some(flush), ..Default::default() };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol = generate_protocol::<ab_slot::GblSlotProtocol>(&efi_entry, &mut ab);
            let mut persist = |_: &mut [u8]| Ok(());
            let mut test_ops = TestGblOps::new(protocol);
            let mut gbl = Gbl::<TestGblOps>::new(&mut test_ops);
            let cursor = gbl.load_slot_interface(&mut persist).unwrap();
            assert!(cursor.ctx.mark_boot_attempt().is_ok());

            assert_eq!(cursor.ctx.mark_boot_attempt(), Err(Error::OperationProhibited));
        });
    }

    #[test]
    fn test_get_max_retries() {
        // SAFETY: verifies that `meta` is properly aligned and not null.
        // It is the caller's responsibility to make sure that `meta` points to
        // a valid GblEfiSlotMetadataBlock.
        unsafe extern "efiapi" fn load_boot_data(
            _: *mut GblEfiABSlotProtocol,
            meta: *mut GblEfiSlotMetadataBlock,
        ) -> EfiStatus {
            if meta.is_null() || !meta.is_aligned() {
                return EFI_STATUS_INVALID_PARAMETER;
            }

            let meta_block = GblEfiSlotMetadataBlock {
                unbootable_metadata: 1,
                max_retries: 66,
                slot_count: 32, // why not?
                merge_status: 0,
            };

            unsafe { *meta = meta_block };
            EFI_STATUS_SUCCESS
        }

        run_test(|image_handle, systab_ptr| {
            let mut ab = GblEfiABSlotProtocol {
                load_boot_data: Some(load_boot_data),
                flush: Some(flush),
                ..Default::default()
            };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol = generate_protocol::<ab_slot::GblSlotProtocol>(&efi_entry, &mut ab);
            let mut persist = |_: &mut [u8]| Ok(());
            let mut test_ops = TestGblOps::new(protocol);
            let mut gbl = Gbl::<TestGblOps>::new(&mut test_ops);
            let cursor = gbl.load_slot_interface(&mut persist).unwrap();
            assert_eq!(cursor.ctx.get_max_retries().unwrap(), 66usize.into());
        });
    }

    #[test]
    fn test_set_active_slot() {
        extern "efiapi" fn set_active_slot(_: *mut GblEfiABSlotProtocol, idx: u8) -> EfiStatus {
            // This is deliberate: we want to make sure that other logic catches
            // 'no such slot' first but we also want to verify that errors propagate.
            if idx != 2 {
                EFI_STATUS_SUCCESS
            } else {
                EFI_STATUS_INVALID_PARAMETER
            }
        }

        run_test(|image_handle, systab_ptr| {
            let mut ab = GblEfiABSlotProtocol {
                get_slot_info: Some(get_info),
                set_active_slot: Some(set_active_slot),
                flush: Some(flush),
                ..Default::default()
            };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol = generate_protocol::<ab_slot::GblSlotProtocol>(&efi_entry, &mut ab);
            let mut persist = |_: &mut [u8]| Ok(());
            let mut test_ops = TestGblOps::new(protocol);
            let mut gbl = Gbl::<TestGblOps>::new(&mut test_ops);
            let cursor = gbl.load_slot_interface(&mut persist).unwrap();

            assert_eq!(cursor.ctx.set_active_slot('b'.into()), Ok(()));
            assert_eq!(cursor.ctx.set_active_slot('c'.into()), Err(Error::Other(None)));

            let bad_suffix = '$'.into();
            assert_eq!(cursor.ctx.set_active_slot(bad_suffix), Err(Error::InvalidInput));
        });
    }

    #[test]
    fn test_set_slot_unbootable() {
        extern "efiapi" fn set_slot_unbootable(
            _: *mut GblEfiABSlotProtocol,
            idx: u8,
            _: u32,
        ) -> EfiStatus {
            // Same thing here as with set_active_slot.
            // We want to make sure that iteration over the slots
            // catches invalid suffixes, but we also want to make sure
            // that errors from the protocol percolate up.
            if idx == 0 {
                EFI_STATUS_SUCCESS
            } else {
                EFI_STATUS_INVALID_PARAMETER
            }
        }

        run_test(|image_handle, systab_ptr| {
            let mut ab = GblEfiABSlotProtocol {
                get_slot_info: Some(get_info),
                set_slot_unbootable: Some(set_slot_unbootable),
                flush: Some(flush),
                ..Default::default()
            };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol = generate_protocol::<ab_slot::GblSlotProtocol>(&efi_entry, &mut ab);
            let mut persist = |_: &mut [u8]| Ok(());
            let mut test_ops = TestGblOps::new(protocol);
            let mut gbl = Gbl::<TestGblOps>::new(&mut test_ops);
            let cursor = gbl.load_slot_interface(&mut persist).unwrap();

            assert_eq!(
                cursor.ctx.set_slot_unbootable('a'.into(), UnbootableReason::SystemUpdate),
                Ok(())
            );

            assert_eq!(
                cursor.ctx.set_slot_unbootable('b'.into(), UnbootableReason::UserRequested),
                Err(Error::InvalidInput)
            );
        });
    }

    #[test]
    fn test_oneshot() {
        // SAFETY:
        // `mode` must point to non-null u32 buffer available to write.
        unsafe extern "efiapi" fn get_boot_mode(
            _: *mut GblEfiABSlotProtocol,
            mode: *mut u32,
        ) -> EfiStatus {
            if mode.is_null() || !mode.is_aligned() {
                return EFI_STATUS_INVALID_PARAMETER;
            }

            unsafe { *mode = BOOT_MODE.with(|r| r.load(Ordering::Relaxed)) };

            EFI_STATUS_SUCCESS
        }

        extern "efiapi" fn set_boot_mode(_: *mut GblEfiABSlotProtocol, mode: u32) -> EfiStatus {
            BOOT_MODE.with(|r| r.store(mode, Ordering::Relaxed));
            EFI_STATUS_SUCCESS
        }

        run_test(|image_handle, systab_ptr| {
            let mut ab = GblEfiABSlotProtocol {
                get_boot_mode: Some(get_boot_mode),
                set_boot_mode: Some(set_boot_mode),
                flush: Some(flush),
                ..Default::default()
            };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol = generate_protocol::<ab_slot::GblSlotProtocol>(&efi_entry, &mut ab);
            let mut persist = |_: &mut [u8]| Ok(());
            let mut test_ops = TestGblOps::new(protocol);
            let mut gbl = Gbl::<TestGblOps>::new(&mut test_ops);
            let cursor = gbl.load_slot_interface(&mut persist).unwrap();

            assert_eq!(
                cursor.ctx.set_oneshot_status(OneShot::Continue(RecoveryTarget::Dedicated)),
                Err(Error::OperationProhibited)
            );
            assert_eq!(cursor.ctx.set_oneshot_status(OneShot::Bootloader), Ok(()));
            assert_eq!(cursor.ctx.get_oneshot_status(), Some(OneShot::Bootloader));
        });
    }
}
