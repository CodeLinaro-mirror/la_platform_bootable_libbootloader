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
//!
//! # Features
//!
//! ## mocks
//!
//! Enabling the `mocks` feature adds the `#[mockall::automock]` attribute
//! to the protocol traits if desired for testing.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

#[rustfmt::skip]
pub mod defs;
pub mod protocol;
pub mod status;
pub mod tpl;

pub use defs::*;

impl EfiGuid {
    /// Returns a new `[EfiGuid]` using the given data.
    pub const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8usize]) -> Self {
        EfiGuid { data1, data2, data3, data4 }
    }
}

/// Any object that can be identified by a GUID.
pub trait Identified {
    /// The UEFI GUID.
    const GUID: EfiGuid;
}

macro_rules! flag_impls {
    ($enum_type:tt) => {
        impl core::ops::BitAnd for $enum_type {
            type Output = Self;
            fn bitand(self, rhs: Self) -> Self::Output {
                Self(self.0 & rhs.0)
            }
        }

        impl core::ops::BitAndAssign for $enum_type {
            fn bitand_assign(&mut self, rhs: Self) {
                self.0 &= rhs.0
            }
        }

        impl core::ops::BitOr for $enum_type {
            type Output = Self;
            fn bitor(self, rhs: Self) -> Self::Output {
                Self(self.0 | rhs.0)
            }
        }

        impl core::ops::BitOrAssign for $enum_type {
            fn bitor_assign(&mut self, rhs: Self) {
                self.0 |= rhs.0
            }
        }
    };
}

flag_impls!(EfiMemoryAttribute);
flag_impls!(GblEfiPartitionBufferFlag);
