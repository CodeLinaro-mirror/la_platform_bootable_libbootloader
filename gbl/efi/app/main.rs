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

//! The GBL UEFI application.
//!
//! This just contains the minimal entry point and global hook declarations
//! needed for a full application build; all the logic should go in the
//! `gbl_efi` library instead.

#![no_std]
#![no_main]

#[cfg(target_arch = "riscv64")]
mod riscv64;

use cfg_if::cfg_if;
use core::{ffi::c_void, panic::PanicInfo};
use efi::{initialize, panic, EfiAllocator};
use efi_types::EfiSystemTable;
use gbl_efi::app_main;

use libstack::initialize_canary;

#[panic_handler]
fn handle_panic(p_info: &PanicInfo) -> ! {
    panic(p_info)
}

#[no_mangle]
#[global_allocator]
static mut EFI_GLOBAL_ALLOCATOR: EfiAllocator = EfiAllocator::new();

/// Pull in the sysdeps required by libavb so the linker can find them.
extern crate avb_sysdeps;
/// Pull in the sysdeps required by boringssl so the linker can find them.
extern crate boringssl_sysdeps;

/// EFI application entry point. Does not return.
///
/// # Safety
/// `image_handle` and `systab_ptr` must be valid objects that adhere to the UEFI specification.
#[no_mangle]
pub unsafe extern "C" fn efi_main(image_handle: *mut c_void, systab_ptr: *mut EfiSystemTable) {
    // TODO(b/411227922): use platform specific tricks to generate a random canary.
    let canary;
    cfg_if! {
        if #[cfg(target_pointer_width = "64")] {
            canary = 0x27085dc5dd4d6b7d;
        } else if #[cfg(target_pointer_width = "32")] {
            canary = 0x612826c7;
        } else {
            compile_error!("Stack canaries require size_of::<usize>() >= 4");
        }
    }
    // SAFETY:
    // * `initialize_canary` is called before any stack-protected function that returns.
    // * `systab_ptr` is non-NULL because this is a just-started UEFI application.
    // * `initialize_canary` has sole access to static mutable variables.
    // * UEFI is single-threaded, so there can be no concurrent
    //   mutations to static, mutable variables.
    //
    // Note: the mask clears the two least significant bytes,
    // and `usize::to_le` ensures that those null bytes are on the left.
    unsafe {
        initialize_canary(systab_ptr, usize::to_le(canary & !0xFFFF));
    }
    // SAFETY:
    // * caller provides valid `image_handle` and `systab_ptr` objects
    // * we only call `initialize()` once
    let entry = unsafe { initialize(image_handle, systab_ptr) }.unwrap();
    app_main(entry).unwrap();
    loop {}
}
