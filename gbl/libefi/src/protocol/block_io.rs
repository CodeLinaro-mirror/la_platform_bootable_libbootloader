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

//! Rust wrapper for `EFI_BLOCK_IO_PROTOCOL`.
use crate::protocol::{MaybeVersioned, Revision};
use efi_types::{
    defs::{EfiBlockIoProtocol, EFI_BLOCK_IO_PROTOCOL_REVISION},
    protocol::Client,
};

impl MaybeVersioned for EfiBlockIoProtocol {
    const REVISION: Option<Revision> =
        Some(Revision::from_u32(EFI_BLOCK_IO_PROTOCOL_REVISION as u32));

    fn revision(&self) -> Option<Revision> {
        Some(self.revision.into())
    }
}

/// Use the [Client] implementation for `BLOCK_IO_PROTOCOL`.
pub type BlockIoProtocol = Client<EfiBlockIoProtocol>;
