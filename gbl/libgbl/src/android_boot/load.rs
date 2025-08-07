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

use super::{cstr_bytes_to_str, BootBuffer};
use crate::{
    android_boot::avf::DEFAULT_PVMFW_PART_NAME_CSTR,
    constants::{FDT_ALIGNMENT, KERNEL_ALIGNMENT, PAGE_SIZE},
    decompress::decompress_kernel,
    gbl_println,
    ops::GblOps,
    partition::RAW_PARTITION_NAME_LEN,
};
use arrayvec::ArrayString;
use avb::SlotVerifyData;
use bootimg::{defs::*, BootImage, VendorImageHeader};
use core::{
    cmp::max,
    ffi::CStr,
    fmt::Write,
    ops::{Deref, Range},
};
use liberror::Error;
use libutils::aligned_subslice;
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
    /// init_boot image,
    pub init_boot: &'a [u8],
    /// Vendor boot image.
    pub vendor_boot: &'a [u8],
    /// Vendor commandline,
    pub vendor_cmdline: &'a str,
    /// Vendor commandline,
    pub vendor_bootconfig: &'a [u8],
    /// DTB.
    pub dtb: &'a [u8],
    /// DTB from partition.
    pub dtb_part: &'a [u8],
    /// pVM firmware image.
    pub pvmfw: &'a [u8],
}

impl LoadedImages<'_> {
    pub(crate) fn bootconfig_supported(&self) -> bool {
        // Bootconfig is introduced after Android 12. A tricky issue is that both Android 11 and
        // 12+ can use boot v3 and vendor boot v3 combination, which makes them indistinguishable.
        // For now, we conservatively assume that boot v3 + vendor_boot v3 does not support
        // bootconfig and therefore needs to add bootconfig to FDT cmdline.
        match BootImage::parse(&self.boot_hdr[..]).unwrap() {
            BootImage::V0(_) | BootImage::V1(_) | BootImage::V2(_) => return false,
            BootImage::V3(_) => match VendorImageHeader::parse(self.vendor_boot).unwrap() {
                // Note: Presence of init_boot implies Android 13.
                VendorImageHeader::V3(_) if self.init_boot.is_empty() => false,
                _ => true,
            },
            BootImage::V4(_) => return true,
        }
    }
}

impl<'a> Default for LoadedImages<'a> {
    fn default() -> Self {
        Self {
            boot_hdr: &[][..],
            dtbo: &[][..],
            boot_cmdline: "",
            init_boot: &[][..],
            vendor_boot: &[][..],
            vendor_cmdline: "",
            vendor_bootconfig: &[][..],
            dtb: &[][..],
            dtb_part: &[][..],
            pvmfw: &[][..],
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
/// * `slot`: The target slot to loader.
/// * `unlocked`: The unlock state.
/// * `verify_data`: `SlotVerifyData` returns from `avb_slot_verify`.
/// * `load`: The destination image assembly load buffer.
pub(super) fn android_load_verified<'a, 'b, 'c>(
    ops: &mut impl GblOps<'a, 'b>,
    slot: u8,
    unlocked: bool,
    verify_data: &'c SlotVerifyData,
    loader: &mut BootBufferLoader<'_>,
) -> Result<LoadedImages<'c>, Error> {
    let mut images = LoadedImages::default();
    images.dtb_part = get_verified_partition(ops, c"dtb", slot, unlocked, true, verify_data)?;
    images.dtbo = get_verified_partition(ops, c"dtbo", slot, unlocked, true, verify_data)?;
    if ops.avf_is_supported()? {
        let pvmfw = DEFAULT_PVMFW_PART_NAME_CSTR;
        images.pvmfw = get_verified_partition(ops, pvmfw, slot, unlocked, true, verify_data)?;
    }
    let boot = get_verified_partition(ops, c"boot", slot, unlocked, false, verify_data)?;
    images.boot_hdr = boot;
    match log_and_parse_bootimg(ops, boot)? {
        BootImage::V3(_) | BootImage::V4(_) => {
            load_v3_and_v4_verified(ops, boot, slot, unlocked, verify_data, loader, &mut images)
        }
        BootImage::V0(_) | BootImage::V1(_) | BootImage::V2(_) => {
            load_v2_or_lower_verified(ops, boot, loader, &mut images)
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
    loader: &mut BootBufferLoader<'_>,
    images: &mut LoadedImages<'c>,
) -> Result<(), Error> {
    let info = BootImageV2Info::new(boot).unwrap();
    images.boot_cmdline = info.cmdline;
    images.dtb = get_range(boot, &info.dtb_range)?;
    loader.kernel_load(ops, get_range(boot, &info.kernel_range)?)?;
    loader.ramdisk_append(get_range(boot, &info.ramdisk_range)?)
}

/// Loads android boot images of version 3 and 4 from avb verified partitions.
///
/// # Args
///
/// * `ops`: An implementation of `GblOps`.
/// * `boot`: A buffer containing the boot image.
/// * `slot`: The target slot to loader.
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
    loader: &mut BootBufferLoader<'_>,
    images: &mut LoadedImages<'c>,
) -> Result<(), Error> {
    let boot_info = BootImageV3Info::new(boot).unwrap();
    images.boot_cmdline = BootImageV3Info::cmdline(boot)?;

    // Loads vendor_boot partition, including ramdisk, dtb, commandline etc.
    let vendor_boot =
        get_verified_partition(ops, c"vendor_boot", slot, unlocked, false, verify_data)?;
    images.vendor_boot = &vendor_boot[..];
    let vendor_boot_info = VendorBootImageInfo::new(vendor_boot)?;
    images.vendor_cmdline = VendorBootImageInfo::cmdline(vendor_boot)?;
    images.dtb = get_range(vendor_boot, &vendor_boot_info.dtb_range)?;
    images.vendor_bootconfig = get_range(vendor_boot, &vendor_boot_info.bootconfig_range)?;

    loader.kernel_load(ops, get_range(boot, &boot_info.kernel_range)?)?;

    loader.ramdisk_append(get_range(vendor_boot, &vendor_boot_info.ramdisk_range)?)?;

    // Finds and loads vendor_kernel_boot partition if provided.
    let vendor_kernel_boot =
        get_verified_partition(ops, c"vendor_kernel_boot", slot, unlocked, true, verify_data)?;
    if vendor_kernel_boot.len() > 0 {
        let info = VendorBootImageInfo::new(vendor_kernel_boot)?;
        // DTB should be provided by vendor_kerenl_boot if it exists.
        images.dtb = get_range(vendor_kernel_boot, &info.dtb_range)?;
        loader.ramdisk_append(get_range(vendor_kernel_boot, &info.ramdisk_range)?)?;
    }

    // Loads generic ramdisk, which may come from either boot or init_boot.
    let generic_ramdisk = match boot_info.ramdisk_range.is_empty() {
        true => {
            images.init_boot =
                get_verified_partition(ops, c"init_boot", slot, unlocked, false, verify_data)?;
            images.init_boot
        }
        false => boot,
    };
    let generic_ramdisk_range = BootImageV3Info::new(generic_ramdisk)?.ramdisk_range;
    loader.ramdisk_append(get_range(generic_ramdisk, &generic_ramdisk_range)?)
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
pub(crate) fn aligned_tail_offset(
    buffer: &[u8],
    size: usize,
    align: usize,
) -> Result<usize, Error> {
    let off = SafeNum::from(buffer.len()) - size;
    let rem = buffer[off.try_into()?..].as_ptr() as usize % align;
    Ok(usize::try_from(off - rem)?)
}

/// Parses and returns the kernel image from a boot image.
pub fn get_kernel(boot: &[u8]) -> Result<&[u8], Error> {
    match BootImage::parse(&boot[..]).map_err(Error::from)? {
        BootImage::V3(_) | BootImage::V4(_) => boot.get(BootImageV3Info::new(boot)?.kernel_range),
        _ => boot.get(BootImageV2Info::new(boot)?.kernel_range),
    }
    .ok_or(Error::InvalidInput)
}

/// Helper data strucuture for loading boot images into a `BootBuffer`.
///
///
/// Each image is loaded to its designated buffers if provided. If not, it'll be loaded to
/// `loader.general` according to the following layout:
///
/// +---------------------------+
/// | ramdisk                   |
/// +---------------------------+
/// | bootconfig                |
/// +---------------------------+
/// | FDT                       |
/// +---------------------------+
/// | kernel                    |
/// +---------------------------+
///
/// Unused image segments will be empty.
#[derive(Default)]
pub(super) struct BootBufferLoader<'a> {
    bufs: BootBuffer<'a>,

    pub(super) ramdisk_sz: usize,
    pub(super) bootconfig_sz: usize,
    pub(super) kernel_sz: usize,

    general_ramdisk: Range<usize>,
    general_fdt: Range<usize>,
    general_kernel: Range<usize>,
}

impl<'a> BootBufferLoader<'a> {
    pub(super) fn new(bufs: BootBuffer<'a>) -> Self {
        Self { bufs, ..Default::default() }
    }

    /// Decompresses and loads kernel into the kernel buffer.
    fn kernel_load<'b, 'c>(
        &mut self,
        ops: &mut impl GblOps<'b, 'c>,
        kernel: &[u8],
    ) -> Result<(), Error> {
        self.general_kernel = self.bufs.general.len()..self.bufs.general.len();
        self.kernel_sz = match self.bufs.kernel.as_mut() {
            // Designated buffer, decompresses directly into it.
            Some(v) => decompress_kernel(ops, kernel, v)?,
            // Use general buffer. decompresses at the tail of the buffer.
            _ => {
                // We assume entire general buffer is available, which means other components
                // shouldn't be loaded yet.
                assert_eq!(self.general_ramdisk, 0..0);
                assert_eq!(self.bootconfig_sz, 0);
                assert_eq!(self.general_fdt, 0..0);
                let sz = decompress_kernel(ops, kernel, self.bufs.general)?;
                // TODO(b/430068343): We place the kenrel at the tail because we haven't fixup
                // FDT/bootconfig yet and don't know their exact size. However the fixup requires
                // calling sync_partition_buffer() first which requires all partition buffers to
                // be dropped. Thus we need to be done with partitions first.
                //
                // Investigate if partition drop can be relaxed (i.e. adding a read-only mode for
                // sync_partition_buffer()) so that this can be avoided.
                let off = aligned_tail_offset(self.bufs.general, sz, KERNEL_ALIGNMENT)?;
                self.bufs.general.copy_within(0..sz, off);
                self.general_kernel = off..off + sz;
                sz
            }
        };
        Ok(())
    }

    /// Appends ramdisks to the ramdisk buffer.
    fn ramdisk_append(&mut self, ramdisk: &[u8]) -> Result<(), Error> {
        match self.bufs.ramdisk.as_mut() {
            Some(v) => split(&mut v[self.ramdisk_sz..], ramdisk.len())?.0.clone_from_slice(ramdisk),
            _ => {
                // We assume all buffer before kernel is available. Thus bootconfig/fdt must not be
                // loaded.
                assert_eq!(self.bootconfig_sz, 0);
                assert_eq!(self.general_fdt, 0..0);
                let load = &mut self.bufs.general[..self.general_kernel.start][self.ramdisk_sz..];
                split(load, ramdisk.len())?.0.clone_from_slice(ramdisk);
                self.general_ramdisk.end = self.ramdisk_sz + ramdisk.len();
            }
        }
        self.ramdisk_sz += ramdisk.len();
        Ok(())
    }

    /// Expands bootconfig buffer to cover the rest of unused space and returns it.
    pub(super) fn expand_bootconfig_buffer(&mut self) -> Result<&mut [u8], Error> {
        self.check_general_fdt_range();
        match self.bufs.ramdisk.as_mut() {
            Some(v) => Ok(&mut v[self.ramdisk_sz..]),
            _ => {
                // Moves FDT to the right most position to make more space for bootconfig fixup.
                let buffer = &mut self.bufs.general[..self.general_kernel.start];
                let off = aligned_tail_offset(buffer as _, self.general_fdt.len(), FDT_ALIGNMENT)?;
                buffer.copy_within(self.general_fdt.clone(), off);
                self.general_fdt = off..off + self.general_fdt.len();
                Ok(&mut self.bufs.general[self.ramdisk_sz..off])
            }
        }
    }

    /// Sets bootconfig size.
    pub(super) fn set_bootconfig_size(&mut self, sz: usize) {
        self.bootconfig_sz = sz;
    }

    /// Computes the end of bootconfig segment in the general load buffer.
    fn general_bootconfig_end(&self) -> Result<usize, Error> {
        match self.bufs.ramdisk.is_some() {
            // Designated ramdisk. The segment is unused in general load buffer.
            true => Ok(0),
            _ => Ok((SafeNum::from(self.general_ramdisk.end) + self.bootconfig_sz).try_into()?),
        }
    }

    /// Returns the designated fdt buffer and the unused buffer between bootconfig and kernel in the
    /// general load buffer for constructing FDT.
    ///
    /// Note: the unused buffer is still necessary even if we have designated buffer because we
    /// need it for relocating DT sources.
    pub(super) fn get_fdt_and_general_unused_buffer(
        &mut self,
    ) -> Result<(Option<&mut [u8]>, &mut [u8]), Error> {
        let unused_off = self.general_bootconfig_end()?;
        let fdt = self.bufs.fdt.as_mut().map(|v| v as _);
        Ok((fdt, &mut self.bufs.general[unused_off..self.general_kernel.start]))
    }

    /// Sets FDT range.
    ///
    /// The intended usage is to get the buffer using `Self::get_fdt_and_general_unused_buffer()`,
    /// construct FDT, compute the ptr range, release the borrow then call this API.
    pub(super) fn set_fdt_range(&mut self, fdt: Range<*const u8>) {
        // Updates `self.general_fdt` if fdt is loaded to general load buffer.
        if self.bufs.fdt.is_none() {
            self.general_fdt = sub_slice_range(&self.bufs.general.as_ptr_range(), &fdt).unwrap();
            self.check_general_fdt_range();
        }
    }

    /// Expands FDT buffer to cover the rest of unused space.
    pub(super) fn expand_fdt(&mut self) -> Result<&mut [u8], Error> {
        // Computes the values that require borrowing self first.
        let bootconfig_end = self.general_bootconfig_end()?;
        self.check_general_fdt_range();
        match self.bufs.fdt.as_mut() {
            // Noop for desiganted FDT buffer.
            Some(v) => Ok(&mut v[..]),
            _ => {
                // Moves FDT to the left most position.
                self.general_fdt =
                    move_left(self.bufs.general, &self.general_fdt, bootconfig_end, FDT_ALIGNMENT)?;
                Ok(&mut self.bufs.general[self.general_fdt.start..self.general_kernel.start])
            }
        }
    }

    /// Checks Self::general_fdt_range is a valid.
    fn check_general_fdt_range(&self) {
        // Either empty or valid range between bootconfig and kernel
        assert!(
            self.general_fdt.len() == 0
                || self.general_fdt.start >= self.general_bootconfig_end().unwrap()
                    && self.general_fdt.end <= self.general_kernel.start
        );
    }

    /// Moves kernel to the left to reserve more space after it.
    pub(super) fn move_kernel_left(&mut self) {
        // Kernel may require additional memory after it for stuffs such as bss section. If kernel
        // is loaded in general buffer, moves it forward to reserve as much space as possible after
        // it.
        if self.bufs.kernel.is_none() {
            let bound = max(self.general_bootconfig_end().unwrap(), self.general_fdt.end);
            self.general_kernel =
                move_left(self.bufs.general, &self.general_kernel, bound, KERNEL_ALIGNMENT)
                    .unwrap();
        }
    }

    /// Splits out the [ramdisk, fdt, kernel, unused] buffers without consuming the loader.
    pub(super) fn splits(&mut self) -> [&mut [u8]; 4] {
        let bufs = BootBuffer {
            general: &mut self.bufs.general[..],
            kernel: self.bufs.kernel.as_mut().map(|v| v as _),
            ramdisk: self.bufs.ramdisk.as_mut().map(|v| v as _),
            fdt: self.bufs.fdt.as_mut().map(|v| v as _),
        };
        BootBufferLoader {
            bufs,
            ramdisk_sz: self.ramdisk_sz,
            bootconfig_sz: self.bootconfig_sz,
            kernel_sz: self.kernel_sz,
            general_ramdisk: self.general_ramdisk.clone(),
            general_fdt: self.general_fdt.clone(),
            general_kernel: self.general_kernel.clone(),
        }
        .into_splits()
    }

    /// Consumes the loader and splits out the [ramdisk, fdt, kernel, unused] buffers
    pub(super) fn into_splits(self) -> [&'a mut [u8]; 4] {
        let (rem, unused) = self.bufs.general.split_at_mut(self.general_kernel.end);
        let (rem, kernel) = rem.split_at_mut(self.general_kernel.start);
        let (ramdisk, fdt) = rem.split_at_mut(self.general_fdt.start);
        // Use designated buffers if provided, otherwise falls back to `general`
        let ramdisk = self.bufs.ramdisk.unwrap_or(ramdisk);
        let fdt = self.bufs.fdt.unwrap_or(fdt);
        let kernel = self.bufs.kernel.unwrap_or(kernel);
        [ramdisk, fdt, kernel, unused]
    }
}

/// Computes the index range of `sub` in the parent slice `buf`.
///
/// Returns None if the `sub` is not a sublice of `buf`
fn sub_slice_range(buf: &Range<*const u8>, sub: &Range<*const u8>) -> Option<Range<usize>> {
    let start = (sub.start as usize).checked_sub(buf.start as _)?;
    let end = (sub.end as usize).checked_sub(buf.start as _)?;
    (sub.end <= buf.end).then_some(start..end)
}

/// Moves data in subslice range `sub` to the left most aligned position after `bound`.
///
/// Returns the new range.
fn move_left(
    buffer: &mut [u8],
    sub: &Range<usize>,
    bound: usize,
    align: usize,
) -> Result<Range<usize>, Error> {
    let range = buffer.as_ptr_range();
    let buffer = aligned_subslice(&mut buffer[..sub.end][bound..], align)?;
    buffer.get(..sub.len()).ok_or(Error::BufferTooSmall(Some(sub.len())))?;
    buffer.copy_within(buffer.len() - sub.len().., 0);
    Ok(sub_slice_range(&range, &buffer[..sub.len()].as_ptr_range()).unwrap())
}
