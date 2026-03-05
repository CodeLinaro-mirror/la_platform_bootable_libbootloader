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

//! GBL Android boot device tree manipulations and related consts exposed.

use crate::{
    android_boot::load::LoadedImages,
    fastboot::boot_items::{BootItem, BootItemContainer},
    gbl_println,
    random::get_random_seed,
    GblOps,
};
use arrayvec::ArrayVec;
use bootparams::commandline::CommandlineBuilder;
use core::ffi::CStr;
use fdt::{Fdt, MAXIMUM_OVERLAYS_TO_APPLY};
#[cfg(feature = "gbl_dev")]
use liberror::Error;
use liberror::Result;
use safemath::SafeNum;

/// Device tree bootargs property to store kernel command line.
pub const PROP_BOOTARGS: &CStr = c"bootargs";
const PROP_BOOTARGS_EXT: &CStr = c"bootargs_ext";
const NODE_CHOSEN: &str = "chosen";

/// Helper function to build DT commandline from loaded images, overlays
/// `bootargs_ext` and additional items provided via fastboot.
pub(crate) fn fdt_build_bootargs<'a, 'b>(
    ops: &mut impl GblOps<'a>,
    fdt: &mut Fdt<&mut [u8]>,
    images: &LoadedImages,
    overlays: impl IntoIterator<Item = &'b [u8]>,
    boot_items: Option<&BootItemContainer>,
    extra_reserved: usize,
) -> Result<()> {
    // We need to obtain all Fdt instances first so that we can later store property slices from
    // them to calculate the expected destination size before copying the data.
    let mut parsed_overlays: ArrayVec<Fdt<&[u8]>, MAXIMUM_OVERLAYS_TO_APPLY> = ArrayVec::new();
    for overlay in overlays {
        parsed_overlays.push(Fdt::new(overlay)?);
    }

    // Reserves 2 for `boot_cmdline` and `vendor_cmdline`.
    let mut bootargs_to_append: ArrayVec<&str, { MAXIMUM_OVERLAYS_TO_APPLY + 2 }> = ArrayVec::new();
    bootargs_to_append.push(images.boot_cmdline);
    bootargs_to_append.push(images.vendor_cmdline);
    // Appends `/chosen/bootargs_ext` from the root of each overlay to `/chosen/bootargs`.
    for overlay in &parsed_overlays {
        if let Ok(bootargs_ext) = overlay.get_property(NODE_CHOSEN, PROP_BOOTARGS_EXT) {
            let bootargs_cstr = CStr::from_bytes_until_nul(bootargs_ext)?;
            bootargs_to_append.push(bootargs_cstr.to_str()?);
        }
    }

    fdt_append_bootargs(ops, fdt, bootargs_to_append)?;
    // Appends `/chosen/bootargs_ext` to `/chosen/bootargs` after overlays are applied:
    // https://source.android.com/docs/core/architecture/dto/optimize#kernel
    //
    // TODO(b/482462468): Remove concat logic once all platforms have switched to using
    // bootargs_ext in the overlay root instead of within fragments.
    fdt_concat_bootargs_properties(fdt, NODE_CHOSEN, PROP_BOOTARGS, PROP_BOOTARGS_EXT)?;
    if let Some(items) = boot_items {
        fdt_append_bootargs(ops, fdt, items.utf8_items(BootItem::Cmdline))?;
    }
    fdt_bootargs_reserve_space(fdt, extra_reserved)?;

    Ok(())
}

/// Helper for appending one or more commandline strings to FDT chosen/bootarg
///
/// # Args
///
/// * `ops`: An implementation of GblOps.
/// * `fdt`: Target FDT to append to.
/// * `cmds`: Commandline strings to add.
pub(crate) fn fdt_append_bootargs<'a, 'b>(
    ops: &mut impl GblOps<'a>,
    fdt: &mut Fdt<&mut [u8]>,
    cmds: impl IntoIterator<Item = &'b str> + Clone,
) -> Result<()> {
    let curr = fdt.get_property(NODE_CHOSEN, PROP_BOOTARGS).map(|v| v.len()).unwrap_or(0);
    let cmds_len = cmds.clone().into_iter().map(|v| v.len() + 1).sum::<usize>();
    let total = curr + cmds_len + 1;
    let buffer = fdt.set_property_placeholder(NODE_CHOSEN, PROP_BOOTARGS, total)?;
    let mut builder = CommandlineBuilder::new_from_prefix(&mut buffer[..])?;
    for v in cmds {
        // The commandline to be added may be from bootconfig which allows ":=". Emit a warning
        // just in case.
        if v.find(":=").is_some() {
            gbl_println!(ops, "{v},  \":=\" assignment may not be supported");
        }
        builder.add_raw(v)?;
    }

    // It has been observed that some OS call `from_utf8` on the entire bootarg buffer to decode,
    // which will pick up everything after the null terminator. Thus zeroize the remaining to
    // prevent OS from trying to think they are valid data.
    builder.zeroize_remains();
    Ok(())
}

/// Reserves additional space in the FDT bootargs property.
fn fdt_bootargs_reserve_space(fdt: &mut Fdt<&mut [u8]>, reserved: usize) -> Result<()> {
    let current_len = fdt.get_property(NODE_CHOSEN, PROP_BOOTARGS)?.len();
    let extended: usize = (SafeNum::from(current_len) + reserved).try_into()?;
    let updated = fdt.set_property_placeholder(NODE_CHOSEN, PROP_BOOTARGS, extended)?;
    let mut builder = CommandlineBuilder::new_from_prefix(&mut updated[..])?;
    builder.zeroize_remains();
    Ok(())
}

/// Appends bootargs properties within the single FDT node.
///
/// This implementation performs the concatenation within the FDT buffer without
/// requiring temporary buffers.
fn fdt_concat_bootargs_properties(
    fdt: &mut Fdt<&mut [u8]>,
    path: &str,
    dst_name: &CStr,
    src_name: &CStr,
) -> Result<()> {
    // If src doesn't exist, we have nothing to concat.
    let Ok(src_prop) = fdt.get_property(path, src_name) else {
        return Ok(());
    };
    let src_len = CStr::from_bytes_until_nul(src_prop)?.to_bytes().len();
    if src_len == 0 {
        return Ok(());
    }

    let dst_prop = fdt.get_property(path, dst_name)?;
    let mut dst_len = CStr::from_bytes_until_nul(dst_prop)?.to_bytes().len();

    let needs_separator = dst_len > 0;
    let separator_len = match needs_separator {
        true => 1,
        _ => 0,
    };
    // Includes null terminator.
    let required_len: usize = (SafeNum::from(dst_len) + separator_len + src_len + 1).try_into()?;

    let fdt_start = fdt.as_ref().as_ptr().addr();
    let (src_offset, dst_offset) = if dst_prop.len() < required_len {
        // Extends the property to required length.
        let dst_off =
            fdt.set_property_placeholder(path, dst_name, required_len)?.as_ptr().addr() - fdt_start;
        let src_off = fdt.get_property(path, src_name)?.as_ptr().addr() - fdt_start;
        (src_off, dst_off)
    } else {
        (src_prop.as_ptr().addr() - fdt_start, dst_prop.as_ptr().addr() - fdt_start)
    };

    let fdt_buf = fdt.as_mut();
    // Replaces null terminator with separator if needed.
    if needs_separator {
        fdt_buf[dst_offset + dst_len] = b' ';
        dst_len += 1;
    }

    // Includes null terminator.
    fdt_buf.copy_within(src_offset..src_offset + src_len + 1, dst_offset + dst_len);

    Ok(())
}

/// RNG seed DT property name.
pub(crate) const RNG_SEED_PROP: &CStr = c"rng-seed";
/// KALSR seed DT property name.
pub(crate) const KASLR_SEED_PROP: &CStr = c"kaslr-seed";

/// The minimal sufficient RNG seed is 32 bytes. The Linux kernel considers up
/// to 512 bytes for this property. Providing 64 bytes as a reasonable balanced
/// option. Could be re-visited.
pub const RNG_SEED_SIZE_BYTES: usize = 64;
/// https://www.kernel.org/doc/Documentation/devicetree/bindings/chosen.txt
pub const KASLR_SEED_SIZE_BYTES: usize = core::mem::size_of::<u64>();

/// Helper function that utilizes device RNG capabilities to provide initial entropy
/// to HLOS and initialize the KASLR feature.
pub(crate) fn fdt_propagate_random<'a>(
    ops: &mut impl GblOps<'a>,
    dt: &mut Fdt<&mut [u8]>,
) -> Result<()> {
    fdt_propagate_random_seed(ops, dt, NODE_CHOSEN, RNG_SEED_PROP, RNG_SEED_SIZE_BYTES)?;
    fdt_propagate_random_seed(ops, dt, NODE_CHOSEN, KASLR_SEED_PROP, KASLR_SEED_SIZE_BYTES)?;

    Ok(())
}

fn fdt_propagate_random_seed<'a>(
    ops: &mut impl GblOps<'a>,
    dt: &mut Fdt<&mut [u8]>,
    path: &str,
    name: &CStr,
    seed_size: usize,
) -> Result<()> {
    // Allocate space for seed data right inside device tree.
    let buffer = dt.set_property_placeholder(path, name, seed_size)?;

    match get_random_seed(ops, buffer) {
        Ok(_) => Ok(()),
        Err(e) => {
            // Error occurred, so clear out seed placeholder.
            dt.delete_property(path, name)?;

            match e {
                #[cfg(feature = "gbl_dev")]
                Error::Unsupported | Error::NotFound | Error::NotReady => {
                    gbl_println!(
                        ops,
                        "SECURITY WARNING: RNG capabilities aren't available: {e}. \
                        Skip KASLR and kernel entropy initialization since DEV GBL flow is used."
                    );
                    Ok(())
                }
                e => {
                    gbl_println!(
                        ops,
                        "SECURITY ERROR: RNG generation is reported an error: {e}. \
                        Cannot initialize KASLR and kernel entropy."
                    );
                    Err(e)
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use libtestutils::AlignedBuffer;

    enum BootargsConcatResult<'a> {
        Ok(&'a [u8]),
        Error,
    }

    enum FdtCapacity {
        Expanded,
        Shrunk,
    }

    fn test_fdt_concat_bootargs_properties<'a>(
        dst: Option<&[u8]>,
        src: Option<&[u8]>,
        capacity: FdtCapacity,
        expected: BootargsConcatResult<'a>,
    ) {
        let mut buffer = AlignedBuffer::new(1024, 8);
        let mut fdt = Fdt::new_empty(&mut buffer[..]).unwrap();

        if let Some(val) = dst {
            fdt.set_property("/", c"dst", val).unwrap();
        }
        if let Some(val) = src {
            fdt.set_property("/", c"src", val).unwrap();
        }

        // DT cannot be extended after shrink.
        if matches!(capacity, FdtCapacity::Shrunk) {
            fdt.shrink_to_fit().unwrap();
        }

        let res = fdt_concat_bootargs_properties(&mut fdt, "/", c"dst", c"src");

        match expected {
            BootargsConcatResult::Ok(expected_val) => {
                res.unwrap();
                assert_eq!(fdt.get_property("/", c"dst").unwrap(), expected_val);
            }
            BootargsConcatResult::Error => {
                res.unwrap_err();
            }
        }
    }

    #[test]
    fn test_fdt_concat_bootargs_properties_success() {
        test_fdt_concat_bootargs_properties(
            Some(b"val1\0"),
            Some(b"val2\0"),
            FdtCapacity::Expanded,
            BootargsConcatResult::Ok(b"val1 val2\0"),
        );
    }

    #[test]
    fn test_fdt_concat_bootargs_properties_no_extend() {
        test_fdt_concat_bootargs_properties(
            Some(b"val1\0\0\0\0\0"),
            Some(b"val2\0"),
            // DT is shrunk, but we have enough space to append.
            FdtCapacity::Shrunk,
            BootargsConcatResult::Ok(b"val1 val2\0"),
        );
    }

    #[test]
    fn test_fdt_concat_bootargs_properties_no_space() {
        test_fdt_concat_bootargs_properties(
            Some(b"val1\0"),
            Some(b"val2\0"),
            // DT is shrunk, so not enough space to append.
            FdtCapacity::Shrunk,
            BootargsConcatResult::Error,
        );
    }

    #[test]
    fn test_fdt_concat_bootargs_properties_src_invalid_cstr() {
        test_fdt_concat_bootargs_properties(
            Some(b"val1\0"),
            Some(b"val2"),
            FdtCapacity::Expanded,
            BootargsConcatResult::Error,
        );
    }

    #[test]
    fn test_fdt_concat_bootargs_properties_dst_invalid_cstr() {
        test_fdt_concat_bootargs_properties(
            Some(b"val1"),
            Some(b"val2\0"),
            FdtCapacity::Expanded,
            BootargsConcatResult::Error,
        );
    }

    #[test]
    fn test_fdt_concat_bootargs_properties_src_missing() {
        test_fdt_concat_bootargs_properties(
            Some(b"val1\0"),
            None,
            FdtCapacity::Shrunk,
            BootargsConcatResult::Ok(b"val1\0"),
        );
    }

    #[test]
    fn test_fdt_concat_bootargs_properties_src_empty() {
        test_fdt_concat_bootargs_properties(
            Some(b"val1\0"),
            Some(b"\0"),
            FdtCapacity::Shrunk,
            BootargsConcatResult::Ok(b"val1\0"),
        );
    }

    #[test]
    fn test_fdt_concat_bootargs_properties_dst_empty() {
        test_fdt_concat_bootargs_properties(
            Some(b"\0"),
            Some(b"val2\0"),
            FdtCapacity::Expanded,
            BootargsConcatResult::Ok(b"val2\0"),
        );
    }

    #[test]
    fn test_fdt_concat_bootargs_properties_dst_missing() {
        test_fdt_concat_bootargs_properties(
            None,
            Some(b"val2\0"),
            FdtCapacity::Expanded,
            BootargsConcatResult::Error,
        );
    }
}
