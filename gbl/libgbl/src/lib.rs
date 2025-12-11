// Copyright 2023, The Android Open Source Project
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

//! # Generic Boot Loader (gbl) Library
//!
//! TODO: b/312610098 - add documentation.
//!
//! The intended users of this library are firmware, bootloader, and bring-up teams at OEMs and SOC
//! Vendors
//!
//! This library is `no_std` as it is intended for use in bootloaders that typically will not
//! support the Rust standard library. However, it does require `alloc` with a global allocator,
//! currently used for:
//! * libavb
//! * kernel decompression

#![feature(never_type)]
#![cfg_attr(not(any(test, android_dylib)), no_std)]
#![allow(async_fn_in_trait)]
extern crate avb;
extern crate core;
extern crate gbl_storage;
#[cfg(feature = "fuchsia")]
extern crate zbi;

pub mod android_boot;
pub mod constants;
pub mod decompress;
pub mod device_tree;
pub mod error;
pub mod fastboot;
#[cfg(feature = "fuchsia")]
pub mod fuchsia_boot;
pub mod gbl_avb;
pub mod misc;
pub mod ops;
pub mod partition;
pub mod random;
pub mod slots;

pub use avb::Descriptor;
pub use error::{IntegrationError, Result};
pub use ops::{GblOps, Os};

#[cfg(test)]
mod tests {
    extern crate avb_sysdeps;
    extern crate avb_test;
    extern crate boringssl_sysdeps;
    extern crate libc_deps_posix;

    use std::{fs, path::Path};

    pub(crate) const TEST_PERMANENT_ATTRIBUTES_PATH: &str = "cert_permanent_attributes.bin";
    pub(crate) const TEST_PERMANENT_ATTRIBUTES_HASH_PATH: &str = "cert_permanent_attributes.hash";

    /// Returns the contents of a test data file.
    ///
    /// Panicks if the requested file cannot be read.
    ///
    /// # Arguments
    /// * `path`: file path relative to libgbl's `testdata/` directory.
    pub(crate) fn testdata(path: &str) -> Vec<u8> {
        let full_path = Path::new("external/gbl+/libgbl/testdata").join(path);
        fs::read(full_path).unwrap()
    }
}
