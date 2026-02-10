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
use fastboot::{LockState, LockType};

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
    /// Indicates if the device critical lock is unlocked.
    pub is_unlocked_critical: bool,
    /// Indicates if a dm-verity error has been detected.
    pub is_dm_verity_error: bool,
    /// Indicates if the device is unlockable.
    pub is_unlockable: bool,
}

impl AvbDeviceStatus {
    /// Returns whether the indicated lock is locked or unlocked.
    pub const fn lock_state(&self, lock_type: LockType) -> LockState {
        let unlocked = match lock_type {
            LockType::Device => self.is_unlocked,
            LockType::Critical => self.is_unlocked_critical,
        };

        if unlocked {
            LockState::Unlocked
        } else {
            LockState::Locked
        }
    }
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

/// The different kinds of verification that can be requested for `SpecializedPartition`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Verification {
    /// The partition must exist and must be successfully verified by vbmeta.
    Required,
    /// Only verify the partition if it exists and has a hash descriptor in the vbmeta blob.
    IfExists,
}

/// A partition which has specialized handling requirements.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SpecializedPartition {
    name_buffer: [u8; PARTITION_NAME_MAX_SIZE],
    /// How to verify this partition, or `None` if this partition does not need to be verified.
    pub verification: Option<Verification>,
    /// Whether this partition is linked to FDR.
    pub fdr: bool,
    /// Whether this partition should be critically-locked. Critically-locked partitions must
    /// not be modifiable from fastboot while the critical lock is set.
    pub critical: bool,
}

impl SpecializedPartition {
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

/// Represents a partition to load and verify during boot.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub enum LoadPartition<'a> {
    /// boot
    Boot,
    /// vendor_boot
    VendorBoot,
    /// vendor_kernel_boot
    VendorKernelBoot,
    /// init_boot,
    InitBoot,
    /// dtb
    Dtb,
    /// dtbo
    Dtbo,
    /// pVM firmware data
    Pvmfw,
    /// Platform specific partition.
    ///
    /// It's up to the caller to ensure that:
    /// * `SpecializedPartition.name_buffer` is valid nul-terminated UTF-8
    /// * `SpecializedPartition.verification` is `Some`
    /// or else trying to use this partition later will panic.
    PlatformSpecific(&'a SpecializedPartition),
}

impl<'a> LoadPartition<'a> {
    /// Returns slotless partition name as &str.
    /// Panics if a `PlatformSpecific` partition has an invalid `name`.
    pub fn name(&self) -> &'a str {
        self.name_cstr().to_str().unwrap()
    }

    /// Returns slotless partition name as &CStr.
    /// Panics if a `PlatformSpecific` partition has an invalid `name`.
    pub fn name_cstr(&self) -> &'a CStr {
        match self {
            Self::Boot => c"boot",
            Self::VendorBoot => c"vendor_boot",
            Self::VendorKernelBoot => c"vendor_kernel_boot",
            Self::InitBoot => c"init_boot",
            Self::Dtb => c"dtb",
            Self::Dtbo => c"dtbo",
            Self::Pvmfw => c"pvmfw",
            Self::PlatformSpecific(part) => part.name_cstr(),
        }
    }

    /// Returns verification type.
    /// Panics if a `PlatformSpecific` partition is not requesting verification.
    pub fn verification(&self) -> Verification {
        match self {
            Self::PlatformSpecific(part) => part.verification.unwrap(),
            // Treat all standard partitions as optional. The loading logic will decide which ones
            // are actually required.
            _ => Verification::IfExists,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_requested_partition_default_is_empty() {
        assert_eq!(SpecializedPartition::default().name_cstr(), c"");
    }

    #[test]
    fn test_requested_partition_exposed_buffer_len() {
        assert_eq!(
            SpecializedPartition::default().name_buffer_mut().len(),
            PARTITION_NAME_MAX_SIZE - 3
        );
    }

    #[test]
    fn test_requested_partition_form_partition_name() {
        let mut partition = SpecializedPartition::default();
        let partition_name = b"boot";

        let slice = partition.name_buffer_mut();
        slice[..partition_name.len()].copy_from_slice(partition_name);
        // Null terminate.
        slice[partition_name.len()] = 0;

        assert_eq!(partition.name_cstr(), c"boot");
    }
}
