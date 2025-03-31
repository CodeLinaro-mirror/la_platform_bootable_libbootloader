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

//! Rust wrapper for `GBL_EFI_AVF_PROTOCOL`.

use crate::protocol::ProtocolInfo;
use efi_types::{EfiGuid, GblEfiAvfProtocol};

/// `GBL_EFI_AVF_PROTOCOL` implementation.
pub struct GblAvfProtocol;

impl ProtocolInfo for GblAvfProtocol {
    type InterfaceType = GblEfiAvfProtocol;

    const GUID: EfiGuid =
        EfiGuid::new(0xe7f1c4a6, 0x0a52, 0x4f61, [0xbd, 0x98, 0x9e, 0x60, 0xb5, 0x59, 0x45, 0x2a]);
}
