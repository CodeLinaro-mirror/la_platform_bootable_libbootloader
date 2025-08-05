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

//! Rust wrapper for `EFI_TIMESTAMP_PROTOCOL`.

use crate::efi_call;
use crate::protocol::{Protocol, ProtocolInfo, Requirement};
use efi_types::{EfiGuid, EfiTimestampProperties, EfiTimestampProtocol};
use liberror::{Error, Result};

/// `EFI_TIMESTAMP_PROTOCOL` implementation.
pub struct TimestampProtocol;

impl ProtocolInfo for TimestampProtocol {
    type InterfaceType = EfiTimestampProtocol;

    const GUID: EfiGuid =
        EfiGuid::new(0xafbfde41, 0x2e6e, 0x4262, [0xba, 0x65, 0x62, 0xb9, 0x23, 0x6e, 0x54, 0x95]);

    const REQUIREMENT: Requirement = Requirement::Optional;
}

// Protocol interface wrappers.
impl Protocol<'_, TimestampProtocol> {
    /// Wraps `EFI_TIMESTAMP_PROTOCOL.get_timestamp()`.
    pub fn get_timestamp(&self) -> Result<u64> {
        // SAFETY:
        // * `get_timestamp.as_ref()` makes sure we are calling a non-null UEFI function pointer,
        //   which is valid assuming correct UEFI implementation.
        Ok(unsafe { self.interface().get_timestamp.as_ref().ok_or(Error::NotFound)?() })
    }

    /// Wraps `EFI_TIMESTAMP_PROTOCOL.get_properties()`.
    pub fn get_properties(&self) -> Result<EfiTimestampProperties> {
        let mut res = Default::default();
        // SAFETY:
        // * `res` points to a valid object and is for output only. It will not be retained by the
        //   API.
        unsafe { efi_call!(self.interface().get_properties, &mut res)? };
        Ok(res)
    }
}
