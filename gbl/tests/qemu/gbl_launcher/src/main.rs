// Copyright 2026, The Android Open Source Project
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

//! GBL launcher application for QEMU.

#![no_std]
#![no_main]

use efi::{efi_println, initialize, EfiAllocator};
use efi_types::{EfiHandle, EfiSystemTable};

#[unsafe(no_mangle)]
#[global_allocator]
static mut EFI_GLOBAL_ALLOCATOR: EfiAllocator = EfiAllocator::new();

/// Pull in the sysdeps required by libavb so the linker can find them.
extern crate avb_sysdeps;
/// Pull in the sysdeps required by boringssl so the linker can find them.
extern crate boringssl_sysdeps;

/// EFI application main entry.
///
/// # Safety
///
/// The caller must provide valid `image_handle` and `systab_ptr` objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn efi_main(image_handle: EfiHandle, systab_ptr: *mut EfiSystemTable) {
    // SAFETY:
    // * caller provides valid `image_handle` and `systab_ptr` objects
    // * we only call `initialize()` once
    let Ok(entry) = (unsafe { initialize(image_handle, systab_ptr) }) else {
        return;
    };
    efi_println!(entry, "Launcher started");
    // TODO(b/499359597): Mock EFI protocols and launch GBL.
    entry.system_table().runtime_services().shutdown().unwrap();
}

#[panic_handler]
fn handle_panic(_p_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
