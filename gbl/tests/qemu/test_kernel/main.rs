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

use core::alloc::{GlobalAlloc, Layout};

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

#[no_mangle]
pub extern "C" fn kernel_main() -> ! {
    semihosting::println!("GBL Custom Kernel loaded and self-relocated successfully from Rust!");

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
