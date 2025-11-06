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

use crate::random::get_random_seed;
use crate::{gbl_println, GblOps};
use core::ffi::CStr;
use fdt::Fdt;
#[cfg(feature = "gbl_dev")]
use liberror::Error;
use liberror::Result;

/// The minimal sufficient RNG seed is 32 bytes. The Linux kernel considers up
/// to 512 bytes for this property. Providing 64 bytes as a reasonable balanced
/// option. Could be re-visited.
pub const RNG_SEED_SIZE_BYTES: usize = 64;
/// https://www.kernel.org/doc/Documentation/devicetree/bindings/chosen.txt
pub const KASLR_SEED_SIZE_BYTES: usize = core::mem::size_of::<u64>();

/// Helper function that utilizes device RNG capabilities to provide initial entropy
/// to HLOS and initialize the KASLR feature.
pub fn propagate_random_into_dt<'a, 'b, T>(
    ops: &mut impl GblOps<'a, 'b>,
    dt: &mut Fdt<T>,
) -> Result<()>
where
    T: AsMut<[u8]> + AsRef<[u8]>,
{
    propagate_random_seed_into_dt(ops, dt, "/chosen", c"rng-seed", RNG_SEED_SIZE_BYTES)?;
    propagate_random_seed_into_dt(ops, dt, "/chosen", c"kaslr-seed", KASLR_SEED_SIZE_BYTES)?;

    Ok(())
}

fn propagate_random_seed_into_dt<'a, 'b, T>(
    ops: &mut impl GblOps<'a, 'b>,
    dt: &mut Fdt<T>,
    path: &str,
    name: &CStr,
    seed_size: usize,
) -> Result<()>
where
    T: AsMut<[u8]> + AsRef<[u8]>,
{
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
