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

//! Rust wrapper for `EFI_LOADED_IMAGE_PROTOCOL`.

use crate::DeviceHandle;
use crate::{
    protocol::{device_path::DevicePathProtocol, Protocol, ProtocolInfo, Requirement},
    versioned_protocol,
};
use core::ffi::c_void;
use core::ptr::{null_mut, NonNull};
use efi_types::{EfiGuid, EfiLoadedImageProtocol, EFI_LOADED_IMAGE_PROTOCOL_REVISION};
use liberror::{Error, Result};

/// EFI_LOADED_IMAGE_PROTOCOL
pub struct LoadedImageProtocol;

versioned_protocol!(LoadedImageProtocol, EFI_LOADED_IMAGE_PROTOCOL_REVISION);

impl ProtocolInfo for LoadedImageProtocol {
    type InterfaceType = EfiLoadedImageProtocol;

    const GUID: EfiGuid =
        EfiGuid::new(0x5b1b31a1, 0x9562, 0x11d2, [0x8e, 0x3f, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b]);

    const REQUIREMENT: Requirement = Requirement::Optional;
}

impl<'a> Protocol<'a, LoadedImageProtocol> {
    /// Wraps `EFI_LOADED_IMAGE_PROTOCOL.DeviceHandle`.
    pub fn device_handle(&self) -> DeviceHandle {
        DeviceHandle(self.interface().device_handle)
    }

    /// Wraps `EFI_LOADED_IMAGE_PROTOCOL.FilePath`.
    pub fn file_path(&self) -> Result<Protocol<'_, DevicePathProtocol>> {
        // SAFETY: `EFI_LOADED_IMAGE_PROTOCOL.FilePath` (if non-null) points to a
        // `EFI_DEVICE_PATH_PROTOCOL` which outlives the `EFI_LOADED_IMAGE_PROTOCOL` itself.
        Ok(unsafe {
            Protocol::new(
                DeviceHandle::new(null_mut()),
                NonNull::new(self.interface().file_path).ok_or(Error::NotFound)?,
                self.efi_entry,
            )
        })
    }

    /// Returns the `EFI_LOADED_IMAGE_PROTOCOL.image_base` field.
    pub fn image_base(&self) -> usize {
        self.interface().image_base as _
    }

    /// Sets `EFI_LOADED_IMAGE_PROTOCOL.LoadOptions` and `LoadOptionsSize`.
    ///
    /// # Safety
    ///
    /// If `options` is non-null, it must point to valid memory of at least `options_size` bytes
    /// that outlives the loaded image execution.
    pub unsafe fn set_load_options(&mut self, options: *const c_void, options_size: u32) {
        let lip = self.interface_ptr();
        // SAFETY: `lip` is a valid protocol interface pointer.
        // Caller guarantees non-null `options` outlives the loaded image execution.
        unsafe {
            (*lip).load_options = options as *mut _;
            (*lip).load_options_size = options_size;
        }
    }
}
