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

//! Mock protocols.
//!
//! The structure of these sub-modules must match the libefi structure so that the code can refer
//! to either one using the same path.

use crate::{DeviceHandle, MOCK_EFI};
use core::{ffi::CStr, fmt::Write};
pub use efi::protocol::gbl_efi_image_loading::EfiImageBufferInfo;
use efi_types::{
    EfiInputKey, EfiTimestampProperties, GblEfiAvbKeyValidationStatus, GblEfiAvbPartition,
    GblEfiAvbVerificationResult, GblEfiFastbootEraseAction, GblEfiImageInfo,
    GblEfiVerifiedDeviceTree,
};
use liberror::Result;
use mockall::mock;

/// Mock `Protocol` type.
pub type Protocol<'a, T> = T;

/// Mock device_path module.
pub mod device_path {
    use super::*;

    mock! {
        /// Mock [efi::DevicePathProtocol].
        pub DevicePathProtocol {}
    }
    /// Map to the libefi name so code under test can just use one name.
    pub type DevicePathProtocol = MockDevicePathProtocol;

    mock! {
        /// Mock [efi::DevicePathToTextProtocol].
        pub DevicePathToTextProtocol {
            /// Returns a [MockDevicePathText].
            ///
            /// Lifetimes are a little difficult to mock perfectly, so here we can only allow a
            /// `'static` return value.
            pub fn convert_device_path_to_text(
                &self,
                device_path: &MockDevicePathProtocol,
                display_only: bool,
                allow_shortcuts: bool,
            ) -> Result<MockDevicePathText<'static>>;
        }
    }
    /// Map to the libefi name so code under test can just use one name.
    pub type DevicePathToTextProtocol = MockDevicePathToTextProtocol;

    mock! {
        /// Mock [efi::DevicePathText].
        pub DevicePathText<'a> {
            /// Returns the text, which is data-only so isn't mocked.
            pub fn text(&self) -> Option<&'a [u16]>;
        }
    }
    /// Map to the libefi name so code under test can just use one name.
    pub type DevicePathText<'a> = MockDevicePathText<'a>;
}

/// Mock loaded_image protocol.
pub mod loaded_image {
    use super::*;

    mock! {
        /// Mock [efi::LoadedImageProtocol].
        pub LoadedImageProtocol {
            /// Returns a real [efi::DeviceHandle], which is data-only so isn't mocked.
            pub fn device_handle(&self) -> DeviceHandle;

            /// Returns the image base address.
            pub fn image_base(&self) -> usize;
        }
    }
    /// Map to the libefi name so code under test can just use one name.
    pub type LoadedImageProtocol = MockLoadedImageProtocol;
}

/// Mock simple_text_input module.
pub mod simple_text_input {
    use super::*;

    mock! {
        /// Mock [efi::SimpleTextInputProtocol].
        pub SimpleTextInputProtocol {
            /// Returns an [EfiInputKey], which is data-only so isn't mocked.
            pub fn read_key_stroke(&self) -> Result<Option<EfiInputKey>>;
        }
    }
    /// Map to the libefi name so code under test can just use one name.
    pub type SimpleTextInputProtocol = MockSimpleTextInputProtocol;
}

/// Mock simple_text_output module.
pub mod simple_text_output {
    use super::*;

    mock! {
        /// Mock [efi::SimpleTextOutputProtocol].
        pub SimpleTextOutputProtocol {}

        impl Write for SimpleTextOutputProtocol {
            fn write_str(&mut self, s: &str) -> core::fmt::Result;
        }
    }
    /// Map to the libefi name so code under test can just use one name.
    pub type SimpleTextOutputProtocol = MockSimpleTextOutputProtocol;

    /// Returns a [MockSimpleTextOutputProtocol] that forwards all calls to `MOCK_EFI`.
    pub fn passthrough_con_out() -> MockSimpleTextOutputProtocol {
        let mut con_out = MockSimpleTextOutputProtocol::default();
        con_out.expect_write_str().returning(|s| {
            MOCK_EFI.with_borrow_mut(|efi| efi.as_mut().unwrap().con_out.write_str(s))
        });
        con_out
    }

    /// While this mock itself isn't necessarily thread-local, passing through to the thread-local
    /// state is our primary use case, so we just disallow [Send] entirely.
    impl !Send for MockSimpleTextOutputProtocol {}
}

/// Mock timestamp protocol
pub mod timestamp {
    use super::*;

    mock! {
        /// Mock [efi::TimestampProtocol]
        pub TimestampProtocol {
            /// Returns the current timestamp.
            pub fn get_timestamp(&self)->Result<u64>;

            /// Returns properties of the timestamp protocol.
            pub fn get_properties(&self)->Result<EfiTimestampProperties>;
        }
    }

    /// Map to the libefi name so code under test can just use one name.
    pub type TimestampProtocol = MockTimestampProtocol;
}

/// Mock image_loading protocol.
pub mod gbl_efi_image_loading {
    use super::*;

    pub use efi::protocol::gbl_efi_image_loading::EfiImageBufferInfo;

    mock! {
        /// Mock [efi::ImageLoadingProtocol].
        pub GblImageLoadingProtocol {
            /// Returns [EfiImageBuffer] matching `gbl_image_info`
            pub fn get_buffer(&self, gbl_image_info: &GblEfiImageInfo) -> Result<EfiImageBufferInfo>;
        }
    }

    /// Map to the libefi name so code under test can just use one name.
    pub type GblImageLoadingProtocol = MockGblImageLoadingProtocol;
}

/// Mock os_configuration protocol.
pub mod gbl_efi_os_configuration {
    use super::*;

    mock! {
        /// Mock [efi::OsConfigurationProtocol].
        pub GblOsConfigurationProtocol {
            /// Wraps `GBL_EFI_OS_CONFIGURATION_PROTOCOL.fixup_kernel_commandline()`
            pub fn fixup_kernel_commandline(
                &self,
                commandline: &CStr,
                fixup: &mut [u8],
            ) -> Result<()>;

            /// Wraps `GBL_EFI_OS_CONFIGURATION_PROTOCOL.fixup_bootconfig()`
            pub fn fixup_bootconfig(
                &self,
                bootconfig: &[u8],
                fixup: &mut [u8],
            ) -> Result<usize>;

            /// Wraps `GBL_EFI_OS_CONFIGURATION_PROTOCOL.select_device_trees()`
            pub fn select_device_trees(
                &self,
                components: &mut [GblEfiVerifiedDeviceTree],
            ) -> Result<()>;
        }
    }

    /// Map to the libefi name so code under test can just use one name.
    pub type GblOsConfigurationProtocol = MockGblOsConfigurationProtocol;
}

/// Mock dt_fixup protocol.
pub mod dt_fixup {
    use super::*;

    mock! {
        /// Mock [efi::DtFixupProtocol].
        pub DtFixupProtocol {
            /// Wraps `EFI_DT_FIXUP_PROTOCOL.revision`.
            pub fn revision(&self) -> u64;

            /// Wraps `EFI_DT_FIXUP_PROTOCOL.fixup()`
            pub fn fixup(&self, device_tree: &mut [u8]) -> Result<()>;
        }
    }

    /// Map to the libefi name so code under test can just use one name.
    pub type DtFixupProtocol = MockDtFixupProtocol;
}

/// Mock avb protocol.
pub mod gbl_efi_avb {
    use super::*;

    /// Mock implementation of `GBL_EFI_AVB_PROTOCOL`.
    /// We use a custom mock implementation instead of relying on `mockall` due to its limitations
    /// regarding argument lifetimes. Specifically, in this case, `mockall` requires the
    /// `validate_vbmeta_public_key.public_key_metadata` argument to have a `'static` lifetime,
    /// which is not practical for our use case.
    #[derive(Clone, Default)]
    pub struct GblAvbProtocol {
        /// Expected return value from `read_partitions_to_verify`.
        pub read_partitions_to_verify_result: Option<Result<usize>>,
        /// Expected return value from `read_device_status`
        pub read_device_status_result: Option<Result<u64>>,
        /// Expected return value from `validate_vbmeta_public_key`.
        pub validate_vbmeta_public_key_result: Option<Result<GblEfiAvbKeyValidationStatus>>,
        /// Expected return value from `read_rollback_index`.
        pub read_rollback_index_result: Option<Result<u64>>,
        /// Expected return value from `write_rollback_index`.
        pub write_rollback_index_result: Option<Result<()>>,
        /// Expected return value from `read_persistent_value`.
        pub read_persistent_value_result: Option<Result<usize>>,
        /// Expected return value from `write_persistent_value`.
        pub write_persistent_value_result: Option<Result<()>>,
    }

    impl GblAvbProtocol {
        /// Wraps `GBL_EFI_AVB_PROTOCOL.read_partitions_to_verify()`.
        pub fn read_partitions_to_verify(
            &self,
            _partitions: &mut [GblEfiAvbPartition],
        ) -> Result<usize> {
            self.read_partitions_to_verify_result.unwrap()
        }

        /// Wraps `GBL_EFI_AVB_PROTOCOL.read_device_status()`.
        pub fn read_device_status(&self) -> Result<u64> {
            self.read_device_status_result.unwrap()
        }

        /// Wraps `GBL_EFI_AVB_PROTOCOL.validate_vbmeta_public_key()`.
        pub fn validate_vbmeta_public_key(
            &self,
            _public_key: &[u8],
            _public_key_metadata: Option<&[u8]>,
        ) -> Result<GblEfiAvbKeyValidationStatus> {
            self.validate_vbmeta_public_key_result.unwrap()
        }

        /// Wraps `GBL_EFI_AVB_PROTOCOL.read_rollback_index()`.
        pub fn read_rollback_index(&self, _index_location: usize) -> Result<u64> {
            self.read_rollback_index_result.unwrap()
        }

        /// Wraps `GBL_EFI_AVB_PROTOCOL.write_rollback_index()`.
        pub fn write_rollback_index(
            &self,
            _index_location: usize,
            _rollback_index: u64,
        ) -> Result<()> {
            self.write_rollback_index_result.unwrap()
        }

        /// Wraps `GBL_EFI_AVB_PROTOCOL.read_persistent_value()`.
        pub fn read_persistent_value(&self, _name: &CStr, _value: &mut [u8]) -> Result<usize> {
            self.read_persistent_value_result.unwrap()
        }

        /// Wraps `GBL_EFI_AVB_PROTOCOL.write_persistent_value()`.
        pub fn write_persistent_value(&self, _name: &CStr, _value: Option<&[u8]>) -> Result<()> {
            self.write_persistent_value_result.unwrap()
        }

        /// Wraps `GBL_EFI_AVB_PROTOCOL.handle_verification_result()`.
        pub fn handle_verification_result(
            &self,
            _verification_result: &GblEfiAvbVerificationResult,
        ) -> Result<()> {
            unimplemented!();
        }
    }
}

/// Mock gbl_efi_fastboot protocol.
pub mod gbl_efi_fastboot {
    use super::*;

    mock! {
        /// Mock [efi::protocol::gbl_efi_fastboot::Var].
        pub Var {
            /// Get name, arguments and corresponding value.
            pub fn get<'s>(&self, out: &mut [u8])
                -> Result<(&'static str, [&'static str; 1], &'static str)>;
        }
    }

    /// Mock [efi::GblFastbootProtocol].
    pub struct GblFastbootProtocol {}

    impl GblFastbootProtocol {
        /// Protocol<'_, GblFastbootProtocol>::get_var.
        pub fn get_var<'a>(
            &self,
            _: &CStr,
            _: impl Iterator<Item = &'a CStr> + Clone,
            _: &mut [u8],
        ) -> Result<usize> {
            unimplemented!()
        }

        /// Protocol<'_, GblFastbootProtocol>::get_var_all.
        pub fn get_var_all(&self, _: impl FnMut(&[&CStr], &CStr)) -> Result<()> {
            unimplemented!()
        }

        /// Protocol<'_, GblFastbootProtocol>::run_oem_function.
        pub fn run_oem_function(
            &self,
            _: &str,
            _: &mut [u8],
            _: impl FnMut(i32, &str) -> Result<()>,
        ) -> Result<()> {
            unimplemented!()
        }

        /// Protocol<'_, GblFastbootProtocol>::get_staged.
        pub fn get_staged(&self, _: &mut [u8]) -> Result<(usize, usize)> {
            unimplemented!()
        }

        /// Protocol<'_, GblFastbootProtocol>::should_stop_in_fastboot.
        pub fn should_stop_in_fastboot(&self) -> bool {
            unimplemented!()
        }

        /// Protocol<'_, GblFastbootProtocol>::set_lock()`
        pub fn set_lock(&self, _: bool, _: bool) -> Result<()> {
            unimplemented!()
        }

        /// Protocol<'_, GblFastbootProtocol>::get_lock()`
        pub fn get_lock(&self, _: bool) -> Result<bool> {
            unimplemented!()
        }

        /// Protocol<'_, GblFastbootProtocol>::vendor_erase()`
        pub fn vendor_erase(&self, _: &str) -> Result<GblEfiFastbootEraseAction> {
            unimplemented!()
        }

        /// Protocol<'_, GblFastbootProtocol>::is_command_allowed()`
        pub fn is_command_allowed<'a>(
            &self,
            _: impl Iterator<Item = &'a CStr> + Clone,
            _: &mut [u8],
            _: &mut [u8],
        ) -> Result<bool> {
            unimplemented!()
        }
    }

    /// Map to the libefi name so code under test can just use one name.
    pub type Var = MockVar;
}

/// Mock gbl_efi_ab_slot
pub mod gbl_efi_ab_slot {
    use super::*;
    use efi::protocol::gbl_efi_ab_slot::GblSlot;
    use efi_types::{GblEfiBootMode, GblEfiSlotMetadataBlock};

    mock! {
        /// Mock of [GblABSlotProtocol]
        pub GblABSlotProtocol {
            /// Mock of GblABSlotProtocol::get_current_slot.
            pub fn get_current_slot(&self) -> Result<GblSlot>;

            /// Mock of GblABSlotProtocol::get_next_slot.
            pub fn get_next_slot(&self, mark_boot_attempt: bool) -> Result<GblSlot>;

            /// Mock of GblABSlotProtocol::load_boot_data.
            pub fn load_boot_data(&self) -> Result<GblEfiSlotMetadataBlock>;

            /// Mock of GblABSlotProtocol::set_active_slot.
            pub fn set_active_slot(&self, idx: u8) -> Result<()>;

            /// Mock of GblABSlotProtocol::set_boot_mode.
            pub fn set_boot_mode(&self, mode: GblEfiBootMode) -> Result<()>;

            /// Mock of GblABSlotProtocol::get_boot_mode.
            pub fn get_boot_mode(&self) -> Result<GblEfiBootMode>;
        }
    }

    /// Map to the libefi name so code under test can just use one name.
    pub type GblABSlotProtocol = MockGblABSlotProtocol;
}

/// Mock gbl_efi_boot_memory
pub mod gbl_efi_boot_memory {
    use super::*;
    use crate::EfiEntry;
    use core::ops::{Deref, DerefMut};
    use efi_types::GblEfiBootBufferType;

    /// Mock GblVendorReservedMemory
    pub struct GblVendorReservedMemory;

    impl GblVendorReservedMemory {
        /// Mock is_preloaded
        pub fn is_preloaded(&self) -> bool {
            unimplemented!()
        }
    }

    impl Deref for GblVendorReservedMemory {
        type Target = [u8];

        fn deref(&self) -> &Self::Target {
            &[][..]
        }
    }

    impl DerefMut for GblVendorReservedMemory {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut [][..]
        }
    }

    /// Mock `gbl_get_partition_buffer`.
    pub fn gbl_get_partition_buffer(_: &EfiEntry, _: &str) -> Result<GblVendorReservedMemory> {
        unimplemented!()
    }

    /// Mock `gbl_sync_partition_buffer`.
    pub fn gbl_sync_partition_buffer(_: &EfiEntry, _: bool) -> Result<()> {
        unimplemented!()
    }

    /// Gets the boot buffer of the given type.
    pub fn gbl_get_boot_buffer(
        _: &EfiEntry,
        _: GblEfiBootBufferType,
        _: usize,
    ) -> Result<GblVendorReservedMemory> {
        unimplemented!();
    }

    /// Mock `gbl_clear_boot_buffer`.
    pub fn gbl_clear_boot_buffer(_: &EfiEntry, _: GblEfiBootBufferType) -> Result<()> {
        unimplemented!();
    }
}

/// Mock avf protocol.
pub mod gbl_efi_avf {
    use super::*;

    mock! {
        /// Mock [efi::AvfProtocol].
        pub GblAvfProtocol {
            /// Wraps `GBL_EFI_AVF_PROTOCOL.read_vendor_dice_handover()`.
            pub fn read_vendor_dice_handover(&self, handover_buffer: &mut [u8]) -> Result<usize>;

            /// Wraps `GBL_EFI_AVF_PROTOCOL.read_secretkeeper_public_key()`.
            pub fn read_secretkeeper_public_key(&self, key_buffer: &mut [u8]) -> Result<usize>;
        }
    }

    /// Map to the libefi name so code under test can just use one name.
    pub type GblAvfProtocol = MockGblAvfProtocol;
}
