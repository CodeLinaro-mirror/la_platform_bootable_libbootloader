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

//! Rust wrapper for `GBL_EFI_BOOT_TARGET_PROTOCOL`.
extern crate libgbl;

use crate::efi_call;
use crate::{
    protocol::{Protocol, ProtocolInfo},
    versioned_protocol,
};
use efi_types::{
    EfiGuid, GblEfiBootTargetProtocol, GblEfiOneShotBootMode, GblEfiSlotInfo,
    GblEfiSlotMetadataBlock, GblEfiUnbootableReason, GBL_EFI_BOOT_TARGET_PROTOCOL_REVISION,
    GBL_EFI_ONE_SHOT_BOOT_MODE_NONE, GBL_EFI_UNBOOTABLE_REASON_NO_MORE_TRIES as NO_MORE_TRIES,
    GBL_EFI_UNBOOTABLE_REASON_SYSTEM_UPDATE as SYSTEM_UPDATE,
    GBL_EFI_UNBOOTABLE_REASON_USER_REQUESTED as USER_REQUESTED,
    GBL_EFI_UNBOOTABLE_REASON_VERIFICATION_FAILURE as VERIFICATION_FAILURE,
};
use liberror::Result;

use libgbl::slots::{Bootability, Slot, Suffix, UnbootableReason};

/// Wraps `GBL_EFI_BOOT_TARGET_PROTOCOL`.
pub struct GblBootTargetProtocol;

versioned_protocol!(GblBootTargetProtocol, GBL_EFI_BOOT_TARGET_PROTOCOL_REVISION);

impl ProtocolInfo for GblBootTargetProtocol {
    type InterfaceType = GblEfiBootTargetProtocol;

    const GUID: EfiGuid =
        EfiGuid::new(0xd382db1b, 0x9ac2, 0x11f0, [0x84, 0xc7, 0x04, 0x7b, 0xcb, 0xa9, 0x60, 0x19]);
}

fn from_efi_unbootable_reason(reason: GblEfiUnbootableReason) -> UnbootableReason {
    match reason {
        NO_MORE_TRIES => UnbootableReason::NoMoreTries,
        SYSTEM_UPDATE => UnbootableReason::SystemUpdate,
        USER_REQUESTED => UnbootableReason::UserRequested,
        VERIFICATION_FAILURE => UnbootableReason::VerificationFailure,
        _ => UnbootableReason::Unknown,
    }
}

/// Newtype around GblEfiSlotInfo to bypass orphan rule.
pub struct GblSlot(pub(crate) GblEfiSlotInfo);

impl From<GblEfiSlotInfo> for GblSlot {
    fn from(slot: GblEfiSlotInfo) -> Self {
        Self(slot)
    }
}

impl TryFrom<GblSlot> for libgbl::slots::Slot {
    type Error = liberror::Error;
    fn try_from(info: GblSlot) -> Result<Self> {
        let info = info.0;
        Ok(Slot {
            suffix: Suffix::try_from(char::try_from(info.suffix)?)?,
            priority: info.priority.into(),
            bootability: match (info.successful, info.tries) {
                (s, _) if s != 0 => Bootability::Successful,
                (0, t) if t > 0 => Bootability::Retriable(info.tries.into()),
                _ => {
                    Bootability::Unbootable(from_efi_unbootable_reason(info.unbootable_reason as _))
                }
            },
        })
    }
}

impl<'a> Protocol<'a, GblBootTargetProtocol> {
    /// Wrapper of `GBL_EFI_BOOT_TARGET_PROTOCOL.load_boot_data()`
    pub fn load_boot_data(&self) -> Result<GblEfiSlotMetadataBlock> {
        let mut block: GblEfiSlotMetadataBlock = Default::default();
        // SAFETY:
        // `self.interface_ptr()` points to a valid object established by `Protocol::new()`.
        // `self.interface_ptr()` is an input parameter and will not be retained. It outlives the call.
        // `block` is an output parameter and will not be retained. It outlives the call.
        unsafe { efi_call!(self.interface().load_boot_data, self.interface_ptr(), &mut block)? }
        Ok(block)
    }

    /// Wrapper of `GBL_EFI_BOOT_TARGET_PROTOCOL.get_slot_info()`
    pub fn get_slot_info(&self, idx: u8) -> Result<GblSlot> {
        let mut info: GblEfiSlotInfo = Default::default();
        // SAFETY:
        // `self.interface_ptr()` points to a valid object established by `Protocol::new()`.
        // `self.interface_ptr()` is an input parameter and will not be retained. It outlives the call.
        // `info` is an output parameter and will not be retained. It outlives the call.
        unsafe { efi_call!(self.interface().get_slot_info, self.interface_ptr(), idx, &mut info,)? }
        Ok(info.into())
    }

    /// Wrapper of `GBL_EFI_BOOT_TARGET_PROTOCOL.get_current_slot()`
    pub fn get_current_slot(&self) -> Result<GblSlot> {
        let mut info: GblEfiSlotInfo = Default::default();
        // SAFETY:
        // `self.interface_ptr()` points to a valid object established by `Protocol::new()`.
        // `self.interface_ptr()` is an input parameter and will not be retained. It outlives the call.
        // `info` is an output parameter and will not be retained. It outlives the call.
        unsafe { efi_call!(self.interface().get_current_slot, self.interface_ptr(), &mut info)? };
        Ok(info.into())
    }

    /// Wrapper of `GBL_EFI_BOOT_TARGET_PROTOCOL.set_active_slot()`
    pub fn set_active_slot(&self, idx: u8) -> Result<()> {
        // SAFETY:
        // `self.interface_ptr()` points to a valid object established by `Protocol::new()`.
        // `self.interface_ptr()` is an input parameter and will not be retained. It outlives the call.
        unsafe { efi_call!(self.interface().set_active_slot, self.interface_ptr(), idx) }
    }

    /// Wrapper of `GBL_EFI_BOOT_TARGET_PROTOCOL.get_one_shot_boot_mode()`
    pub fn get_one_shot_boot_mode(&self) -> Result<GblEfiOneShotBootMode> {
        let mut mode = GBL_EFI_ONE_SHOT_BOOT_MODE_NONE;
        // SAFETY:
        // `self.interface_ptr()` points to a valid object established by `Protocol::new()`.
        // `self.interface_ptr()` is an input parameter and will not be retained. It outlives the call.
        // `mode` is an output parameter and will not be retained. It outlives the call.
        unsafe {
            efi_call!(self.interface().get_one_shot_boot_mode, self.interface_ptr(), &mut mode)?
        };
        Ok(mode)
    }
}
