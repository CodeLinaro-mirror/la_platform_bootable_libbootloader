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

//! Rust wrapper for `EFI_RANDOM_NUMBER_GENERATOR_PROTOCOL`.

use crate::efi_call;
use crate::protocol::{MaybeVersioned, Protocol, ProtocolInfo};
use core::mem::MaybeUninit;
use efi_types::{EfiGuid, EfiRngProtocol};
use liberror::Result;
use zerocopy::FromBytes;

impl MaybeVersioned for EfiRngProtocol {}

/// Wraps `EFI_RANDOM_NUMBER_GENERATOR_PROTOCOL`
pub struct RandomNumberGeneratorProtocol;

impl ProtocolInfo for RandomNumberGeneratorProtocol {
    type InterfaceType = EfiRngProtocol;

    const GUID: EfiGuid =
        EfiGuid::new(0x3152bca5, 0xeade, 0x433d, [0x86, 0x2e, 0xc0, 0x1c, 0xdc, 0x29, 0x1f, 0x44]);
}

impl Protocol<'_, RandomNumberGeneratorProtocol> {
    /// `get_info` is deliberately not provided. See b/413744208 for details.

    /// Wrapper of `EFI_RANDOM_NUMBER_GENERATOR_PROTOCOL.get_rng()`
    ///
    /// Note: the `get_rng` protocol method takes an optional `rng_algorithm` arg.
    ///       Algorithm selection is deliberately not supported for simplicity,
    ///       but support is tracked by b/413744208.
    pub fn get_rng<T: FromBytes>(&self) -> Result<T> {
        let mut raw = MaybeUninit::<T>::uninit();

        // SAFETY:
        // * `self.interface_ptr()` points to a valid object
        //   established by `Protocol::new()`.
        // * `self.interface_ptr()` is an input parameter and will not be retained.
        //   It outlives the call.
        // * `rng_algorithm` is optional. Null is a valid value.
        // * `raw` is a non-null output parameter.
        //   It is not retained, outlives the call, and points to size_of::<T>() bytes
        //   that will be written to.
        unsafe {
            efi_call!(
                self.interface().get_rng,
                self.interface_ptr(),
                core::ptr::null(),
                core::mem::size_of_val(&raw),
                raw.as_mut_ptr() as *mut u8,
            )?;
        }

        // SAFETY:
        // * The EFI call succeeded, so some byte pattern was written to `raw`.
        // * Because T: FromBytes, any byte pattern is a valid `T`.
        Ok(unsafe { raw.assume_init() })
    }
}
