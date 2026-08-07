// Copyright 2026, The Android Open Source Project
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

//! Rust trait for
//! [`EFI_DEVICE_PATH_PROTOCOL`](https://uefi.org/specs/UEFI/2.11/10_Protocols_Device_Path_Protocol.html).

use crate::{
    defs::{EfiDevicePathProtocol, EfiGuid},
    Identified, EFI_DEVICE_PATH_PROTOCOL_GUID_U64_0, EFI_DEVICE_PATH_PROTOCOL_GUID_U64_1,
};

impl Identified for EfiDevicePathProtocol {
    const GUID: EfiGuid = EfiGuid::from_u64s(
        EFI_DEVICE_PATH_PROTOCOL_GUID_U64_0,
        EFI_DEVICE_PATH_PROTOCOL_GUID_U64_1,
    );
}
