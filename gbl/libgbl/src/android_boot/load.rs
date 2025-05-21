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

use super::cstr_bytes_to_str;
use crate::{
    android_boot::avf::DEFAULT_PVMFW_PART_NAME_CSTR,
    constants::{KERNEL_ALIGNMENT, PAGE_SIZE},
    decompress::decompress_kernel,
    gbl_println,
    ops::GblOps,
    partition::RAW_PARTITION_NAME_LEN,
};
use arrayvec::ArrayString;
use avb::SlotVerifyData;
use bootimg::{defs::*, BootImage, VendorImageHeader};
use core::{
    array,
    ffi::CStr,
    fmt::Write,
    ops::{Deref, Range},
};
use liberror::Error;
use safemath::SafeNum;
use zerocopy::{IntoBytes, Ref};

// Represents a slot suffix.
pub(crate) struct SlotSuffix([u8; 3]);

impl SlotSuffix {
    // Creates a new instance.
    pub(crate) fn new(slot: u8) -> Result<Self, Error> {
        let suffix = u32::from(slot) + u32::from(b'a');
        match char::from_u32(suffix).map(|v| v.is_ascii_lowercase()) {
            Some(true) => Ok(Self([b'_', suffix.try_into().unwrap(), 0])),
            _ => Err(Error::Other(Some("Invalid slot index"))),
        }
    }

    // Casts as CStr.
    fn as_cstr(&self) -> &CStr {
        CStr::from_bytes_with_nul(&self.0[..]).unwrap()
    }
}

impl Deref for SlotSuffix {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_cstr().to_str().unwrap()
    }
}

/// Returns a slotted partition name.
pub(crate) fn slotted_part(
    part: &str,
    slot: u8,
) -> Result<ArrayString<RAW_PARTITION_NAME_LEN>, Error> {
    let mut res = ArrayString::new_const();
    write!(res, "{}{}", part, &SlotSuffix::new(slot)? as &str).unwrap();
    Ok(res)
}

// Helper for constructing a range that ends at a page aligned boundary. Specifically, it returns
// `start..round_up(start + sz, page_size)`
fn page_aligned_range(
    start: impl Into<SafeNum>,
    sz: impl Into<SafeNum>,
    page_size: impl Into<SafeNum>,
) -> Result<Range<usize>, Error> {
    let start = start.into();
    Ok(start.try_into()?..(start + sz.into()).round_up(page_size.into()).try_into()?)
}

/// Represents a loaded boot image of version 2 and lower.
///
/// TODO(b/384964561): Investigate if the APIs are better suited for bootimg.rs. The issue
/// is that it uses `Error` and `SafeNum` from GBL.
#[derive(Clone)]
struct BootImageV2Info<'a> {
    cmdline: &'a str,
    kernel_range: Range<usize>,
    ramdisk_range: Range<usize>,
    dtb_range: Range<usize>,
}

impl<'a> BootImageV2Info<'a> {
    /// Creates a new instance.
    fn new(buffer: &'a [u8]) -> Result<Self, Error> {
        let header = BootImage::parse(buffer)?;
        if matches!(header, BootImage::V3(_) | BootImage::V4(_)) {
            return Err(Error::InvalidInput);
        }
        // This is valid since v1/v2 are superset of v0.
        let v0 = Ref::into_ref(Ref::<_, boot_img_hdr_v0>::from_prefix(&buffer[..]).unwrap().0);
        let page_size: usize = v0.page_size.try_into()?;
        let cmdline = cstr_bytes_to_str(&v0.cmdline[..])?;
        let kernel_range = page_aligned_range(page_size, v0.kernel_size, page_size)?;
        let ramdisk_range = page_aligned_range(kernel_range.end, v0.ramdisk_size, page_size)?;
        let second_range = page_aligned_range(ramdisk_range.end, v0.second_size, page_size)?;

        let start = u64::try_from(second_range.end)?;
        let (off, sz) = match header {
            BootImage::V1(v) => (v.recovery_dtbo_offset, v.recovery_dtbo_size),
            BootImage::V2(v) => (v._base.recovery_dtbo_offset, v._base.recovery_dtbo_size),
            _ => (start, 0),
        };
        let recovery_dtb_range = match off >= start {
            true => page_aligned_range(off, sz, page_size)?,
            _ if off == 0 => page_aligned_range(start, 0, page_size)?,
            _ => return Err(Error::Other(Some("Unexpected recovery_dtbo_offset"))),
        };
        let dtb_sz: usize = match header {
            BootImage::V2(v) => v.dtb_size.try_into().unwrap(),
            _ => 0,
        };
        let dtb_range = page_aligned_range(recovery_dtb_range.end, dtb_sz, page_size)?;
        Ok(Self { cmdline, kernel_range, ramdisk_range, dtb_range })
    }
}

// Contains information of a V3/V4 boot image.
#[derive(Clone)]
pub(crate) struct BootImageV3Info {
    pub kernel_range: Range<usize>,
    pub ramdisk_range: Range<usize>,
}

impl BootImageV3Info {
    /// Creates a new instance.
    pub(crate) fn new(buffer: &[u8]) -> Result<Self, Error> {
        let header = BootImage::parse(buffer)?;
        if !matches!(header, BootImage::V3(_) | BootImage::V4(_)) {
            return Err(Error::InvalidInput);
        }
        let v3 = Self::v3(buffer);
        let kernel_range = page_aligned_range(PAGE_SIZE, v3.kernel_size, PAGE_SIZE)?;
        let ramdisk_range = page_aligned_range(kernel_range.end, v3.ramdisk_size, PAGE_SIZE)?;
        Ok(Self { kernel_range, ramdisk_range })
    }

    /// Gets the v3 base header.
    fn v3(buffer: &[u8]) -> &boot_img_hdr_v3 {
        // This is valid since v4 is superset of v3.
        Ref::into_ref(Ref::from_prefix(&buffer[..]).unwrap().0)
    }

    // Decodes the kernel cmdline
    fn cmdline(buffer: &[u8]) -> Result<&str, Error> {
        cstr_bytes_to_str(&Self::v3(buffer).cmdline[..])
    }
}

/// Contains vendor boot image information.
struct VendorBootImageInfo {
    ramdisk_range: Range<usize>,
    dtb_range: Range<usize>,
    bootconfig_range: Range<usize>,
}

impl VendorBootImageInfo {
    /// Creates a new instance.
    fn new(buffer: &[u8]) -> Result<Self, Error> {
        let header = VendorImageHeader::parse(buffer)?;
        let v3 = Self::v3(buffer);
        let page_size = v3.page_size;
        let header_size = match header {
            VendorImageHeader::V3(hdr) => SafeNum::from(hdr.as_bytes().len()),
            VendorImageHeader::V4(hdr) => SafeNum::from(hdr.as_bytes().len()),
        }
        .round_up(page_size);
        let ramdisk_range = page_aligned_range(header_size, v3.vendor_ramdisk_size, page_size)?;
        let dtb_sz: usize = v3.dtb_size.try_into().unwrap();
        let dtb_range = page_aligned_range(ramdisk_range.end, dtb_sz, page_size)?;

        let (table_sz, bootconfig_sz) = match header {
            VendorImageHeader::V4(hdr) => (hdr.vendor_ramdisk_table_size, hdr.bootconfig_size),
            _ => (0, 0),
        };
        let table = page_aligned_range(dtb_range.end, table_sz, page_size)?;
        let bootconfig_range = table.end..(table.end + usize::try_from(bootconfig_sz)?);
        Ok(Self { ramdisk_range, dtb_range, bootconfig_range })
    }

    /// Gets the v3 base header.
    fn v3(buffer: &[u8]) -> &vendor_boot_img_hdr_v3 {
        Ref::into_ref(Ref::<_, _>::from_prefix(&buffer[..]).unwrap().0)
    }

    // Decodes the vendor cmdline
    fn cmdline(buffer: &[u8]) -> Result<&str, Error> {
        cstr_bytes_to_str(&Self::v3(buffer).cmdline[..])
    }
}

/// Contains various loaded image components by `android_load_verify`
pub struct LoadedImages<'a> {
    /// Boot image header.
    pub boot_hdr: &'a [u8],
    /// dtbo image.
    pub dtbo: &'a [u8],
    /// Kernel commandline.
    pub boot_cmdline: &'a str,
    /// Vendor commandline,
    pub vendor_cmdline: &'a str,
    /// Vendor commandline,
    pub vendor_bootconfig: &'a [u8],
    /// DTB.
    pub dtb: &'a [u8],
    /// DTB from partition.
    pub dtb_part: &'a [u8],
    /// Kernel image.
    pub kernel: &'a [u8],
    /// Ramdisk image.
    pub ramdisk: &'a [u8],
    /// pVM firmware image.
    pub pvmfw: &'a [u8],
    /// Unused portion. Can be used by the caller to construct FDT.
    pub unused: &'a mut [u8],
}

impl<'a> Default for LoadedImages<'a> {
    fn default() -> Self {
        Self {
            boot_hdr: &[][..],
            dtbo: &[][..],
            boot_cmdline: "",
            vendor_cmdline: "",
            vendor_bootconfig: &[][..],
            dtb: &[][..],
            dtb_part: &[][..],
            kernel: &[][..],
            ramdisk: &[][..],
            pvmfw: &[][..],
            unused: &mut [][..],
        }
    }
}

/// Helper for getting a successfully verified partition from `SlotVerifyData`
fn get_verified_partition<'a, 'b, 'c>(
    ops: &mut impl GblOps<'a, 'b>,
    part: &CStr,
    slot: u8,
    unlocked: bool,
    optional: bool,
    verify_data: &'c SlotVerifyData,
) -> Result<&'c [u8], Error> {
    let slotted = slotted_part(part.to_str().unwrap(), slot).unwrap();
    let part_res = verify_data.partition_data().iter().find(|v| v.partition_name() == part);
    match part_res {
        None if optional => {
            gbl_println!(ops, "{slotted:?} isn't loaded by avb. Image is optional. Skips.");
            Ok(&[][..])
        }
        None => {
            gbl_println!(
                ops,
                "Error: {slotted:?} is required but isn't loaded by avb. \
                The partition may be missing or not included in the vbmeta."
            );
            Err(Error::NotFound)
        }
        Some(v) => match v.verify_result() {
            Ok(_) => Ok(v.data()),
            Err(_) if unlocked => {
                gbl_println!(ops, "{slotted:?} verification fails. Device is unlocked. Continues.");
                Ok(v.data())
            }
            _ => unreachable!(), // Should not reach here if locked and verification failed.
        },
    }
}

/// Helper for parsing and logging boot image version.
fn log_and_parse_bootimg<'a, 'b, 'c>(
    ops: &mut impl GblOps<'a, 'b>,
    data: &'c [u8],
) -> Result<BootImage<&'c [u8]>, Error> {
    let bootimg = BootImage::parse(&data[..]).map_err(Error::from)?;
    let ver_str = match bootimg {
        BootImage::V0(_) => "V0",
        BootImage::V1(_) => "V1",
        BootImage::V2(_) => "V2",
        BootImage::V3(_) => "V3",
        BootImage::V4(_) => "V4",
    };
    gbl_println!(ops, "Boot image {ver_str}.");
    Ok(bootimg)
}

/// Loads android images from avb verified partitions.
///
/// # Args
///
/// * `ops`: An implementation of `GblOps`.
/// * `slot`: The target slot to load.
/// * `unlocked`: The unlock state.
/// * `verify_data`: `SlotVerifyData` returns from `avb_slot_verify`.
/// * `load`: The destination image assembly load buffer.
pub(crate) fn android_load_verified<'a, 'b, 'c>(
    ops: &mut impl GblOps<'a, 'b>,
    slot: u8,
    unlocked: bool,
    verify_data: &'c SlotVerifyData,
    load: &'c mut [u8],
) -> Result<LoadedImages<'c>, Error> {
    let mut images = LoadedImages::default();
    images.dtb_part = get_verified_partition(ops, c"dtb", slot, unlocked, true, verify_data)?;
    images.dtbo = get_verified_partition(ops, c"dtbo", slot, unlocked, true, verify_data)?;
    let pvmfw = DEFAULT_PVMFW_PART_NAME_CSTR;
    images.pvmfw = get_verified_partition(ops, pvmfw, slot, unlocked, true, verify_data)?;
    let boot = get_verified_partition(ops, c"boot", slot, unlocked, false, verify_data)?;
    images.boot_hdr = boot;
    match log_and_parse_bootimg(ops, boot)? {
        BootImage::V3(_) | BootImage::V4(_) => {
            load_v3_and_v4_verified(ops, boot, slot, unlocked, verify_data, load, &mut images)
        }
        BootImage::V0(_) | BootImage::V1(_) | BootImage::V2(_) => {
            load_v2_or_lower_verified(ops, boot, load, &mut images)
        }
    }?;
    Ok(images)
}

/// Loads android boot images of version 0, 1 and 2 from avb verified partitions.
///
/// # Args
///
/// * `ops`: An implementation of `GblOps`.
/// * `boot`: A buffer containing the boot image loaded by avb.
/// * `load`: The destination image assembly load buffer.
/// * `images`: The output `LoadedImages` that stores partitioned image slices loaded to `load` or
///   `boot` by avb.
///
/// For v0, v1, v2 images:
///
/// * Both kernel and ramdisk come from the boot image.
/// * vendor_boot, init_boot are irrelevant.
fn load_v2_or_lower_verified<'a, 'b, 'c>(
    ops: &mut impl GblOps<'a, 'b>,
    boot: &'c [u8],
    load: &'c mut [u8],
    images: &mut LoadedImages<'c>,
) -> Result<(), Error> {
    // Loads from `verify_data` into the following layout:
    //
    // +------------------------+
    // | ramdisk                |
    // +------------------------+
    // | unused                 |
    // +------------------------+
    // | kernel                 |
    // +------------------------+
    //
    // dtb, cmdline comes directly from avb allocated buffers.
    let info = BootImageV2Info::new(boot).unwrap();
    images.boot_cmdline = info.cmdline;
    images.dtb = get_range(boot, &info.dtb_range)?;
    let (ramdisk, remains) = split(load, info.ramdisk_range.len())?;
    ramdisk.clone_from_slice(get_range(boot, &info.ramdisk_range)?);
    images.ramdisk = ramdisk;
    let (remains, kernel, kernel_sz) =
        relocate_kernel(ops, get_range(boot, &info.kernel_range)?, remains)?;
    images.kernel = &kernel[..kernel_sz];
    images.unused = remains;
    Ok(())
}

/// Loads android boot images of version 3 and 4 from avb verified partitions.
///
/// # Args
///
/// * `ops`: An implementation of `GblOps`.
/// * `boot`: A buffer containing the boot image.
/// * `slot`: The target slot to load.
/// * `unlocked`: The unlock state.
/// * `verify_data`: `SlotVerifyData` returns from `avb_slot_verify`.
/// * `load`: The destination image assembly load buffer.
/// * `images`: The output `LoadedImages` that stores partitioned image slices loaded to `load` or
///   `verify_data` by avb.
///
/// V3, V4 images have the following characteristics:
///
/// * Kernel comes from "boot_a/b" partition.
/// * Generic ramdisk may come from either "boot_a/b" or "init_boot_a/b" partitions.
/// * Vendor ramdisk comes from "vendor_boot_a/b" partition.
/// * V4 vendor_boot contains additional bootconfig.
///
/// From the perspective of Android versions:
///
/// Android 11:
///
/// * Can use v3 header.
/// * Generic ramdisk is in the "boot_a/b" partitions.
///
/// Android 12:
///
/// * Can use v3 or v4 header.
/// * Generic ramdisk is in the "boot_a/b" partitions.
///
/// Android 13:
///
/// * Can use v3 or v4 header.
/// * Generic ramdisk is in the "init_boot_a/b" partitions.
///
/// # References
///
/// https://source.android.com/docs/core/architecture/bootloader/boot-image-header
/// https://source.android.com/docs/core/architecture/partitions/vendor-boot-partitions
/// https://source.android.com/docs/core/architecture/partitions/generic-boot
fn load_v3_and_v4_verified<'a, 'b, 'c>(
    ops: &mut impl GblOps<'a, 'b>,
    boot: &'c [u8],
    slot: u8,
    unlocked: bool,
    verify_data: &'c SlotVerifyData,
    load: &'c mut [u8],
    images: &mut LoadedImages<'c>,
) -> Result<(), Error> {
    // Loads from `verify_data` into the following layout:
    //
    // +------------------------+
    // | vendor ramdisk         |
    // +------------------------+
    // | generic ramdisk        |
    // +------------------------+
    // | unused                 |
    // +------------------------+
    // | kernel                 |
    // +------------------------+
    //
    // vendor dtb, boot config, cmdline come directly from avb allocated buffers.
    let boot_info = BootImageV3Info::new(boot).unwrap();
    images.boot_cmdline = BootImageV3Info::cmdline(boot)?;

    // Loads vendor_boot partition, including ramdisk, dtb, commandline etc.
    let vendor_boot =
        get_verified_partition(ops, c"vendor_boot", slot, unlocked, false, verify_data)?;
    let vendor_boot_info = VendorBootImageInfo::new(vendor_boot)?;
    images.vendor_cmdline = VendorBootImageInfo::cmdline(vendor_boot)?;
    images.dtb = get_range(vendor_boot, &vendor_boot_info.dtb_range)?;
    images.vendor_bootconfig = get_range(vendor_boot, &vendor_boot_info.bootconfig_range)?;
    let (vendor_ramdisk_buf, remains) = load.split_at_mut(vendor_boot_info.ramdisk_range.len());
    vendor_ramdisk_buf.clone_from_slice(get_range(vendor_boot, &vendor_boot_info.ramdisk_range)?);

    // Loads generic ramdisk, which may come from either boot or init_boot.
    let generic_ramdisk = match boot_info.ramdisk_range.is_empty() {
        true => get_verified_partition(ops, c"init_boot", slot, unlocked, false, verify_data)?,
        false => boot,
    };
    let generic_ramdisk_range = BootImageV3Info::new(generic_ramdisk)?.ramdisk_range;
    let (generic_ramdisk_buf, _) = remains.split_at_mut(generic_ramdisk_range.len());
    generic_ramdisk_buf.clone_from_slice(get_range(generic_ramdisk, &generic_ramdisk_range)?);

    let ramdisk_len = vendor_ramdisk_buf.len() + generic_ramdisk_buf.len();
    let (ramdisk, remains) = load.split_at_mut(ramdisk_len);
    images.ramdisk = ramdisk;

    // Loads kernel
    let (remains, kernel, kernel_sz) =
        relocate_kernel(ops, get_range(boot, &boot_info.kernel_range)?, remains)?;
    images.kernel = &kernel[..kernel_sz];
    images.unused = remains;

    Ok(())
}

/// Wrapper of `split_at_mut_checked` with error conversion.
pub(crate) fn split(buffer: &mut [u8], size: usize) -> Result<(&mut [u8], &mut [u8]), Error> {
    buffer.split_at_mut_checked(size).ok_or(Error::BufferTooSmall(Some(size)))
}

/// Wrapper of slice::get with error conversion.
fn get_range<'a>(buffer: &'a [u8], range: &Range<usize>) -> Result<&'a [u8], Error> {
    buffer.get(range.clone()).ok_or(Error::InvalidInput)
}

/// Calculates the offset from the start of the buffer to obtain an aligned tail
/// that can fit at least `size` bytes with the given alignment.
///
/// Returns the starting offset of the aligned tail slice.
fn aligned_tail_offset(buffer: &[u8], size: usize, align: usize) -> Result<usize, Error> {
    let off = SafeNum::from(buffer.len()) - size;
    let rem = buffer[off.try_into()?..].as_ptr() as usize % align;
    Ok(usize::try_from(off - rem)?)
}

/// Splits a buffer into multiple chunks of the given sizes.
///
/// Returns an array of slices corresponding to the given sizes and the remaining slice.
pub(super) fn split_chunks<'a, const N: usize>(
    buf: &'a mut [u8],
    sizes: &[usize; N],
) -> ([&'a mut [u8]; N], &'a mut [u8]) {
    let mut chunks: [_; N] = array::from_fn(|_| &mut [][..]);
    let mut remains = buf;
    for (i, ele) in sizes.iter().enumerate() {
        (chunks[i], remains) = remains.split_at_mut(*ele);
    }
    (chunks, remains)
}

/// A helper function for relocating and decompressing kernel to a different buffer.
///
/// The relocated kernel will be place at the tail.
///
/// Returns the leading unused slice, the relocated slice and the actual kernel size without
/// alignment padding.
fn relocate_kernel<'a, 'b, 'c>(
    ops: &mut impl GblOps<'a, 'b>,
    kernel: &[u8],
    dst: &'c mut [u8],
) -> Result<(&'c mut [u8], &'c mut [u8], usize), Error> {
    let decompressed_size = decompress_kernel(ops, kernel, dst)?;
    let aligned_tail_off = aligned_tail_offset(dst, decompressed_size, KERNEL_ALIGNMENT)?;
    dst.copy_within(0..decompressed_size, aligned_tail_off);
    let (prefix, tail) = split(dst, aligned_tail_off)?;
    Ok((prefix, tail, decompressed_size))
}

/// Parses and returns the kernel image from a boot image.
pub fn get_kernel(boot: &[u8]) -> Result<&[u8], Error> {
    match BootImage::parse(&boot[..]).map_err(Error::from)? {
        BootImage::V3(_) | BootImage::V4(_) => boot.get(BootImageV3Info::new(boot)?.kernel_range),
        _ => boot.get(BootImageV2Info::new(boot)?.kernel_range),
    }
    .ok_or(Error::InvalidInput)
}
