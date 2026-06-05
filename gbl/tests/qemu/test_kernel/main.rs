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

//! Standalone test kernel main loop in Rust.

#![no_std]
#![no_main]

use bootparams::bootconfig::extract_bootconfig;
use core::alloc::{GlobalAlloc, Layout};
use fdt::Fdt;

// Noop allocator to meet dependency requirement.
struct StubAllocator;

// SAFETY: This is a noop allocator.
unsafe impl GlobalAlloc for StubAllocator {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: StubAllocator = StubAllocator;

#[no_mangle]
pub static __stack_chk_guard: usize = 0xffffffff;

#[no_mangle]
pub extern "C" fn __stack_chk_fail() -> ! {
    semihosting::println!("ERROR: Stack protection check failed inside custom test_kernel!");
    semihosting::shutdown(1);
}

#[no_mangle]
pub extern "C" fn rust_eh_personality() {}

/// Linux like kernel main entry
///
/// # Safety
///
/// * Caller must guarantee that `fdt_addr` points to a valid device tree blob.
/// * Caller must guarantee that `linux,initrd-start` and `linux,initrd-end` mark a valid ramdisk
///   address range if specified
#[no_mangle]
pub unsafe extern "C" fn kernel_main(fdt_addr: *const u8) -> ! {
    semihosting::println!("GBL Custom Kernel loaded and self-relocated successfully from Rust!");

    // For now we don't expect fdt to be null.
    assert!(!fdt_addr.is_null());

    // Parses FDT
    // SAFETY: By safety contract, if fdt_addr is not null, it points to a valid dtb.
    let (_, slice) = unsafe { fdt::FdtHeader::from_raw(fdt_addr) }
        .inspect_err(|e| semihosting::println!("Failed to parse fdt {e}"))
        .unwrap();
    let fdt = Fdt::new(slice).unwrap();
    // Extract ramdisk range
    let initrd_start_prop = fdt.get_property("chosen", c"linux,initrd-start").unwrap();
    let initrd_end_prop = fdt.get_property("chosen", c"linux,initrd-end").unwrap();
    let initrd_start = u64::from_be_bytes(initrd_start_prop.try_into().unwrap());
    let initrd_end = u64::from_be_bytes(initrd_end_prop.try_into().unwrap());
    let ramdisk_len = usize::try_from(initrd_end - initrd_start).unwrap();

    // SAFETY: By safety contract, `linux,initrd-start` and `linux,initrd-end` marks a valid
    // ramdisk address range.
    let ramdisk_slice =
        unsafe { core::slice::from_raw_parts(initrd_start as *const u8, ramdisk_len) };
    let bootconfig = extract_bootconfig(ramdisk_slice)
        .inspect_err(|e| semihosting::println!("Failed to extract bootconfig: {:?}", e))
        .unwrap();

    let is_normal = bootconfig
        .rfind("androidboot.force_normal_boot")
        .filter(|v| bootconfig[*v..].starts_with("androidboot.force_normal_boot=1\n"))
        .is_some();
    semihosting::println!("Normal Mode: {is_normal:?}",);

    semihosting::println!("Exiting QEMU test via semihosting.");
    // Terminate QEMU cleanly via libsemihosting
    semihosting::shutdown(0);
}

#[panic_handler]
fn panic(p_info: &core::panic::PanicInfo) -> ! {
    semihosting::println!("Panic! {}", p_info);
    semihosting::shutdown(1);
}

// ELF requires the following symbol.
#[no_mangle]
pub extern "C" fn _Unwind_Resume(_: *mut core::ffi::c_void) {
    panic!();
}
