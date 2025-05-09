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

//! Rust definitions for EFI types, data structures, and methods.
//!
//! This crate aims to have as few dependencies as possible so that it can be
//! used in GBL or ported to different UEFI implementations.

// This is both safe and stable but is being tweaked due to the "system" abi variant
// potentially not supporting varargs.
// See issue #100189 <https://github.com/rust-lang/rust/issues/100189> for more information
#![feature(extended_varargs_abi_support)]
#![cfg_attr(not(test), no_std)]

pub mod defs;
pub mod status;

pub use defs::*;

impl EfiGuid {
    /// Returns a new `[EfiGuid]` using the given data.
    pub const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8usize]) -> Self {
        EfiGuid { data1, data2, data3, data4 }
    }
}

impl GblEfiImageInfo {
    /// Decodes the UCS2 GblEfiImageInfo.ImageType using buffer, and returns &str of UTF8
    /// representation.
    ///
    /// Buffer must be big enough to contain UTF8 representation of the UCS2 image type.
    ///
    /// Maximum image type as UCS2 is PARTITION_NAME_LEN_U16.
    /// And [PARTITION_NAME_LEN_U8] bytes is maximum buffer size needed for UTF8 representation.
    ///
    /// # Result
    /// Ok(&str) - On success return UTF8 representation of the image type.
    /// Err(usize) if provided buffer is too small, with the minimum buffer size as the payload.
    pub fn get_type_str<'a>(&self, buffer_utf8: &'a mut [u8]) -> Result<&'a str, usize> {
        let mut index = 0;
        let chars_iter = char::decode_utf16(self.ImageType.iter().copied())
            .map(|c_res| c_res.unwrap_or(char::REPLACEMENT_CHARACTER))
            .take_while(|c| *c != '\0');
        for c in chars_iter.clone() {
            if c.len_utf8() <= buffer_utf8[index..].len() {
                index += c.encode_utf8(&mut buffer_utf8[index..]).len();
            } else {
                let buffer_min_len = chars_iter.clone().map(char::len_utf8).sum();
                return Err(buffer_min_len);
            }
        }
        // SAFETY:
        // _unchecked should be OK here since we wrote each utf8 byte ourselves,
        // but it's just an optimization, checked version would be fine also.
        unsafe { Ok(core::str::from_utf8_unchecked(&buffer_utf8[..index])) }
    }
}
