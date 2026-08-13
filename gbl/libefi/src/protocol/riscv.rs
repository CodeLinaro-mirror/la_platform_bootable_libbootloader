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

//! Rust wrapper for `RISCV_EFI_BOOT_PROTOCOL`.

use crate::efi_call;
use crate::{
    protocol::{Protocol, ProtocolInfo},
    versioned_protocol,
};
use efi_types::{
    EfiGuid, EfiRiscvBootProtocol, EFI_RISCV_BOOT_PROTOCOL_GUID_U64_0,
    EFI_RISCV_BOOT_PROTOCOL_GUID_U64_1, EFI_RISCV_BOOT_PROTOCOL_REVISION,
};
use liberror::Result;

/// RISCV_EFI_BOOT_PROTOCOL
pub struct RiscvBootProtocol;

versioned_protocol!(RiscvBootProtocol, EFI_RISCV_BOOT_PROTOCOL_REVISION);

impl ProtocolInfo for RiscvBootProtocol {
    type InterfaceType = EfiRiscvBootProtocol;

    const GUID: EfiGuid =
        EfiGuid::from_u64s(EFI_RISCV_BOOT_PROTOCOL_GUID_U64_0, EFI_RISCV_BOOT_PROTOCOL_GUID_U64_1);
}

impl<'a> Protocol<'a, RiscvBootProtocol> {
    /// Wraps `RISCV_EFI_BOOT_PROTOCOL.GetBootHartId()`.
    pub fn get_boot_hartid(&self) -> Result<usize> {
        let mut boot_hart_id: usize = 0;
        // SAFETY:
        // `self.interface_ptr()` guarantees `self.interface_ptr()` is non-null and points to a valid object
        // established by `Protocol::new()`.
        // `self.interface_ptr()` is input parameter and will not be retained. It outlives the call.
        // `&mut boot_hart_id` is output parameter and will not be retained. It outlives the call.
        unsafe {
            efi_call!(self.interface().get_boot_hartid, self.interface_ptr(), &mut boot_hart_id)?;
        }
        Ok(boot_hart_id)
    }
}
