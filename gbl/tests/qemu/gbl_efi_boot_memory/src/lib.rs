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

//! Implementation of the `GblEfiBootMemoryProtocol` for qemu tests.

#![cfg_attr(not(test), no_std)]

use efi_types::{
    defs::{
        GblEfiBootBufferType, GBL_EFI_BOOT_BUFFER_TYPE_GENERAL_LOAD,
        GBL_EFI_BOOT_MEMORY_PROTOCOL_REVISION,
    },
    protocol::gbl_efi_boot_memory::{BootBuffer, GblEfiBootMemorySafe},
    status::{EfiError, EfiResult},
    GblEfiPartitionBufferFlag,
};

/// Test implementation of `GblEfiBootMemory`.
pub struct GblEfiBootMemoryImpl;

impl GblEfiBootMemorySafe for GblEfiBootMemoryImpl {
    type PartitionBuffer = spin::MutexGuard<'static, [u8], spin::Spin>;

    fn revision(&self) -> u64 {
        GBL_EFI_BOOT_MEMORY_PROTOCOL_REVISION
    }

    fn sync_partition_buffer(&self, _: bool) -> EfiResult<()> {
        Ok(())
    }

    fn get_partition_buffer(
        &self,
        _: &core::ffi::CStr,
    ) -> EfiResult<(Self::PartitionBuffer, GblEfiPartitionBufferFlag)> {
        Err(EfiError::NotFound)
    }

    fn take_boot_buffer(&self, buffer_type: GblEfiBootBufferType) -> EfiResult<BootBuffer> {
        match buffer_type {
            GBL_EFI_BOOT_BUFFER_TYPE_GENERAL_LOAD => {
                // GBL by default allocates 256mb of general load buffer, which is too much for
                // test. Instruct GBL to use smaller size using this protocol.
                // TODO(b/499359597): Make the size configurable.
                Ok(BootBuffer::ToAllocate(16 * 1024 * 1024))
            }
            _ => Err(EfiError::NotFound),
        }
    }
}
