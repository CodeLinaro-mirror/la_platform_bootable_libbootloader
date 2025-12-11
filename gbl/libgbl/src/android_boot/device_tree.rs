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

/// Device tree bootargs property to store kernel command line.
pub const PROP_BOOTARGS: &CStr = c"bootargs";
const PROP_BOOTARGS_EXT: &CStr = c"bootargs_ext";

/// Helper function to build DT commandline from loaded images, overlays
/// `bootargs_ext` and additional items provided via fastboot.
pub(crate) fn fdt_build_bootargs<'a>(
    ops: &mut impl GblOps<'a>,
    fdt: &mut Fdt<&mut [u8]>,
    images: &LoadedImages,
    overlays: &[&[u8]],
    boot_items: Option<&BootItemContainer>,
    extra_reserved: usize,
) -> Result<()> {
    // Reserves 2 for `boot_cmdline` and `vendor_cmdline`.
    let mut bootargs_to_append: ArrayVec<&str, { MAXIMUM_OVERLAYS_TO_APPLY + 2 }> = ArrayVec::new();
    bootargs_to_append.push(images.boot_cmdline);
    bootargs_to_append.push(images.vendor_cmdline);

    // Appends `/chosen/bootargs_ext` from `overlays` to the `/chosen/bootargs`:
    // https://source.android.com/docs/core/architecture/dto/optimize#kernel
    for overlay in overlays {
        let overlay = Fdt::new(overlay)?;
        if let Ok(bootargs_ext) = overlay.get_property("/chosen", PROP_BOOTARGS_EXT) {
            let bootargs_cstr = CStr::from_bytes_until_nul(bootargs_ext)?;
            bootargs_to_append.push(bootargs_cstr.to_str()?);
        }
    }

    fdt_append_bootargs(ops, fdt, bootargs_to_append, extra_reserved)?;
    if let Some(items) = boot_items {
        fdt_append_bootargs(ops, fdt, items.utf8_items(BootItem::Cmdline), 0)?;
    }

    Ok(())
}

/// Helper for appending one or more commandline strings to FDT chosen/bootarg
///
/// # Args
///
/// * `ops`: An implementation of GblOps.
/// * `fdt`: Target FDT to append to.
/// * `cmds`: Commandline strings to add.
/// * `extra_reserved`: Additional empty space to add.
pub(crate) fn fdt_append_bootargs<'a, 'b>(
    ops: &mut impl GblOps<'a>,
    fdt: &mut Fdt<&mut [u8]>,
    cmds: impl IntoIterator<Item = &'b str> + Clone,
    extra_reserved: usize,
) -> Result<()> {
    let curr = fdt.get_property("chosen", PROP_BOOTARGS).map(|v| v.len()).unwrap_or(0);
    let cmds_len = cmds.clone().into_iter().map(|v| v.len() + 1).sum::<usize>();
    let total = curr + cmds_len + 1 + extra_reserved;
    let buffer = fdt.set_property_placeholder("chosen", PROP_BOOTARGS, total)?;
    let mut builder = CommandlineBuilder::new_from_prefix(&mut buffer[..])?;
    for v in cmds {
        // The commandline to be added may be from bootconfig which allows ":=". Emit a warning
        // just in case.
        if v.find(":=").is_some() {
            gbl_println!(ops, "{v},  \":=\" assignment may not be supported");
        }
        builder.add(v)?;
    }

    // It has been observed that some OS call `from_utf8` on the entire bootarg buffer to decode,
    // which will pick up everything after the null terminator. Thus zeroize the remaining to
    // prevent OS from trying to think they are valid data.
    builder.zeroize_remains();
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
    fdt_propagate_random_seed(ops, dt, "/chosen", RNG_SEED_PROP, RNG_SEED_SIZE_BYTES)?;
    fdt_propagate_random_seed(ops, dt, "/chosen", KASLR_SEED_PROP, KASLR_SEED_SIZE_BYTES)?;

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
