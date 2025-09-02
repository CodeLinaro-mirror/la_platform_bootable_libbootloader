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

//! This file provides common constants that are used in GBL

// TODO(b/380392958) Cleanup other used of the constants. Move them here as well.

use crate::{android_boot::load::SlotSuffix, partition::RawName};
use arrayvec::ArrayString;
use core::{
    ffi::CStr,
    fmt::{Debug, Display, Formatter, Write},
};
use liberror::Error;
use static_assertions::const_assert_eq;
#[cfg(feature = "fuchsia")]
use zbi::ZBI_ALIGNMENT_USIZE;

/// Macro for defining Kibibyte-sized constants
#[macro_export]
macro_rules! KiB  (
    ($x:expr) => {
        $x*1024
    }
);
const_assert_eq!(KiB!(1), 1024);
const_assert_eq!(KiB!(5), 5 * 1024);

/// Macro for defining Mebibyte-sized constants
#[macro_export]
macro_rules! MiB  (
    ($x:expr) => {
        $x*KiB!(1024)
    }
);
const_assert_eq!(MiB!(1), 1024 * 1024);
const_assert_eq!(MiB!(5), 5 * 1024 * 1024);

pub use KiB;
pub use MiB;

/// Must be synced with the one defined in image loading protocol.
/// https://cs.android.com/android/kernel/superproject/+/common-android-mainline:bootable/libbootloader/gbl/docs/GBL_EFI_IMAGE_LOADING_PROTOCOL.md
pub const IMAGE_NAME_MAX_LEN: usize = 36;

/// Kernel image alignment requirement.
pub const KERNEL_ALIGNMENT: usize = MiB!(2);

/// Zircon Kernel image alignment requirement.
pub const ZIRCON_KERNEL_ALIGNMENT: usize = KiB!(64);

/// FDT image alignment requirement.
pub const FDT_ALIGNMENT: usize = 8;

/// Expected max size for BootCmd zbi item.
pub const BOOTCMD_SIZE: usize = KiB!(16);

/// Page size
pub const PAGE_SIZE: usize = KiB!(4);

/// Pvmfw image alignment requirement.
pub const PVMFW_DATA_ALIGNMENT: usize = PAGE_SIZE;

// Type alias for raw partition image name.
type PartitionImageName = ArrayString<IMAGE_NAME_MAX_LEN>;

/// Image names list.
/// Used for identifying what buffer size/alignment is necessary.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ImageType {
    /// ZBI for Zircon kernel
    #[cfg(feature = "fuchsia")]
    ZbiZircon,
    /// ZBI items
    #[cfg(feature = "fuchsia")]
    ZbiItems,
    /// Boot
    Boot,
    /// FDT
    Fdt,
    /// Ramdisk
    Ramdisk,
    /// pVM firmware data
    PvmfwData,
    /// Raw partition
    Partition(PartitionImageName),
}

impl ImageType {
    /// Get alignment required for the [ImageType]
    pub fn alignment(&self) -> usize {
        match self {
            #[cfg(feature = "fuchsia")]
            Self::ZbiZircon => ZIRCON_KERNEL_ALIGNMENT,
            #[cfg(feature = "fuchsia")]
            Self::ZbiItems => ZBI_ALIGNMENT_USIZE,
            Self::Boot => KERNEL_ALIGNMENT,
            Self::Fdt => FDT_ALIGNMENT,
            Self::Ramdisk => PAGE_SIZE,
            Self::PvmfwData => PVMFW_DATA_ALIGNMENT,
            Self::Partition(_) => PAGE_SIZE,
        }
    }

    /// Get image name for the [ImageType]
    pub fn name(&self) -> &str {
        match self {
            #[cfg(feature = "fuchsia")]
            ImageType::ZbiZircon => "zbi_zircon",
            #[cfg(feature = "fuchsia")]
            ImageType::ZbiItems => "zbi_items",
            ImageType::Boot => "boot",
            ImageType::Fdt => "fdt",
            ImageType::Ramdisk => "ramdisk",
            ImageType::PvmfwData => "pvmfw_data",
            Self::Partition(name) => &name,
        }
    }
}

impl Display for ImageType {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Represents a standard boot partition.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Partition {
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
    // Use our custom `RawName` instead of ArrayString for its better CStr support.
    PlatformSpecific(RawName),
}

impl Partition {
    /// Returns slotless partition name as &str.
    pub fn name(&self) -> &str {
        self.name_cstr().to_str().unwrap()
    }

    /// Returns slotless partition name as &CStr.
    pub fn name_cstr(&self) -> &CStr {
        match self {
            Self::Boot => c"boot",
            Self::VendorBoot => c"vendor_boot",
            Self::VendorKernelBoot => c"vendor_kernel_boot",
            Self::InitBoot => c"init_boot",
            Self::Dtb => c"dtb",
            Self::Dtbo => c"dtbo",
            Self::Pvmfw => c"pvmfw",
            Self::PlatformSpecific(v) => v.to_cstr(),
        }
    }

    /// Returns the slotted name.
    pub fn slotted(&self, slot: u8) -> Result<PartitionImageName, Error> {
        let mut res = ArrayString::new_const();
        write!(res, "{}{}", self.name(), &SlotSuffix::new(slot)? as &str).unwrap();
        Ok(res)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_slotted_image_name() {
        assert_eq!(&Partition::Boot.slotted(0).unwrap(), "boot_a");
        assert_eq!(&Partition::Boot.slotted(1).unwrap(), "boot_b");
    }
}
