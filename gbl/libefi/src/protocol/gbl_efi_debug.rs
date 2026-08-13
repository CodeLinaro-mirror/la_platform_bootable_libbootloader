// Copyright 2025, The Android Open Source Project
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

//! Rust wrapper for `GBL_EFI_DEBUG_PROTOCOL`.
use crate::{
    efi_call,
    protocol::{Protocol, ProtocolInfo, Requirement},
    versioned_protocol,
};
use efi_types::{
    EfiGuid, GblEfiDebugErrorTag, GblEfiDebugProtocol, GBL_EFI_DEBUG_PROTOCOL_GUID_U64_0,
    GBL_EFI_DEBUG_PROTOCOL_GUID_U64_1, GBL_EFI_DEBUG_PROTOCOL_REVISION,
};
use libutils::get_frame_ptr;

/// Wraps `GBL_EFI_DEBUG_PROTOCOL`.
pub struct GblDebugProtocol;

versioned_protocol!(GblDebugProtocol, GBL_EFI_DEBUG_PROTOCOL_REVISION);

impl ProtocolInfo for GblDebugProtocol {
    type InterfaceType = GblEfiDebugProtocol;

    const GUID: EfiGuid =
        EfiGuid::from_u64s(GBL_EFI_DEBUG_PROTOCOL_GUID_U64_0, GBL_EFI_DEBUG_PROTOCOL_GUID_U64_1);

    const REQUIREMENT: Requirement = Requirement::Optional;

    const METRICS_TAG: Option<&'static str> = Some("gbl_debug");
}

impl<'a> Protocol<'a, GblDebugProtocol> {
    /// Wrapper of `GBL_EFI_DEBUG_PROTOCOL.fatal_error()`
    pub fn fatal_error(&self, tag: GblEfiDebugErrorTag) {
        // SAFETY:
        // `self.interface_ptr()` points to a valid object established by `Protocol::new()`.
        // `self.interface_ptr()` is an input parameter and will not be retained. It outlives the call.
        let _ = unsafe {
            efi_call!(
                self.interface().fatal_error,
                self.interface_ptr(),
                get_frame_ptr() as *const core::ffi::c_void,
                tag
            )
        };
    }
}
