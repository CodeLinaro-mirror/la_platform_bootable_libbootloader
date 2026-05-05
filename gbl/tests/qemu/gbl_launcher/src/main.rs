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

extern crate alloc;

use efi::{efi_println, initialize, EfiAllocator};
use efi_types::{EfiHandle, EfiSystemTable};

#[unsafe(no_mangle)]
#[global_allocator]
static mut EFI_GLOBAL_ALLOCATOR: EfiAllocator = EfiAllocator::new();

/// Pull in the sysdeps required by libavb so the linker can find them.
extern crate avb_sysdeps;
/// Pull in the sysdeps required by boringssl so the linker can find them.
extern crate boringssl_sysdeps;

// Embed the test kernel binary at compile-time using include_bytes!
// Path is relative to this main.rs file path:
// TODO(b/509953349): Remove this once kernel is assembled into boot image.
static KERNEL_BYTES: &[u8] = include_bytes!("../test_kernel.bin");

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
    semihosting::println!("Semihosting enabled.");

    let mut file = semihosting::File::open(c"fdt.dtb", semihosting::OpenMode::ReadBinary).unwrap();
    let mut fdt_buffer = alloc::vec![0u8; file.len().unwrap()];
    let read_size = file.read(&mut fdt_buffer).unwrap();
    semihosting::println!("Loaded FDT with {} bytes", read_size);
    // TODO(b/499359597): Mock EFI protocols and launch GBL.

    // TODO(b/509953349): Remove this once kernel is assembled into boot image.
    let mut mmap_buf = alloc::vec![0u8; 65536];
    let _ = efi::exit_boot_services(entry, &mut mmap_buf).unwrap();
    // SAFETY:
    // * `KERNEL_BYTES` is a custom test kernel blob.
    unsafe { boot::aarch64::jump_linux_el2_or_lower(KERNEL_BYTES, &[], &[]) };
}

#[panic_handler]
fn handle_panic(_p_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
