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

//! GBL AVB implementation.

use arrayvec::ArrayVec;
use avb_bindgen::{AVB_MAX_NUMBER_OF_LOADED_PARTITIONS, AVB_PART_NAME_MAX_SIZE};
use core::ffi::CStr;

pub(crate) mod ops;
pub mod state;

/// Maximum partition name length supported by libavb.
pub(crate) const PARTITION_NAME_MAX_SIZE: usize = AVB_PART_NAME_MAX_SIZE as _;

/// Maximum number of partitions that libavb and GBL can verify.
pub(crate) const MAX_PARTITIONS_TO_VERIFY: usize = AVB_MAX_NUMBER_OF_LOADED_PARTITIONS as _;

/// Half of the partitions are reserved for FW-provided entries.
pub(crate) const MAX_REQUESTED_PARTITIONS_TO_VERIFY: usize = MAX_PARTITIONS_TO_VERIFY / 2;

/// Array vector capable of storing up to `MAX_PARTITIONS_TO_VERIFY` items.
pub type ArrayMaxParts<T> = ArrayVec<T, MAX_PARTITIONS_TO_VERIFY>;

/// Array vector capable of storing up to `MAX_REQUESTED_PARTITIONS_TO_VERIFY`
/// items.
pub type ArrayMaxRequestedParts<T> = ArrayVec<T, MAX_REQUESTED_PARTITIONS_TO_VERIFY>;

/// Represents AVB (Android Verified Boot) device status information.
#[derive(Clone, Debug, PartialEq)]
pub struct AvbDeviceStatus {
    /// Indicates if the device is currently in an unlocked state.
    pub is_unlocked: bool,
    /// Indicates if a dm-verity error has been detected.
    pub is_dm_verity_error: bool,
}

/// Represents AVB vbmeta property.
pub struct AvbProperty<'a> {
    /// Name of the source partition.
    pub partition: &'a CStr,
    /// Property key name.
    pub key: &'a CStr,
    /// Property value.
    pub value_with_nul: &'a [u8],
}

/// Represents AVB loaded/verified partition.
pub struct AvbPartition<'a> {
    /// Name of the partition.
    pub name: &'a CStr,
    /// Data of the partition.
    pub data: &'a [u8],
}

/// Partition requested by FW for loading and verification. Contains a name buffer and a
/// flag indicating whether the partition is optional.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct RequestedPartition {
    name_buffer: [u8; PARTITION_NAME_MAX_SIZE],
    /// Indicates the partition is optional to be loaded/verified.
    pub optional: bool,
}

impl RequestedPartition {
    /// Returns a mutable byte slice for the partition name. It is the caller's responsibility
    /// to ensure that only UTF-8 characters are copied into the returned buffer. Otherwise,
    /// the subsequent `name_cstr()` call will cause a panic.
    pub fn name_buffer_mut(&mut self) -> &mut [u8] {
        // Leaving 2 bytes for libavb to append the slot suffix and 1 byte for null termination.
        const RESERVED_SUFFIX_LEN: usize = 3;

        let name_buffer_len = self.name_buffer.len() - RESERVED_SUFFIX_LEN;
        &mut self.name_buffer[..name_buffer_len]
    }

    /// Returns the partition name as a C-style string.
    pub fn name_cstr(&self) -> &CStr {
        CStr::from_bytes_until_nul(&self.name_buffer).unwrap()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_requested_partition_default_is_empty() {
        assert_eq!(RequestedPartition::default().name_cstr(), c"");
    }

    #[test]
    fn test_requested_partition_exposed_buffer_len() {
        assert_eq!(
            RequestedPartition::default().name_buffer_mut().len(),
            PARTITION_NAME_MAX_SIZE - 3
        );
    }

    #[test]
    fn test_requested_partition_form_partition_name() {
        let mut partition = RequestedPartition::default();
        let partition_name = b"boot";

        let slice = partition.name_buffer_mut();
        slice[..partition_name.len()].copy_from_slice(partition_name);
        // Null terminate.
        slice[partition_name.len()] = 0;

        assert_eq!(partition.name_cstr(), c"boot");
    }
}
