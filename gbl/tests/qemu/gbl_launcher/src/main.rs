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

use efi::{
    efi_println, initialize,
    protocol::{
        gbl_efi_boot_control::GblBootControlProtocol, gbl_efi_boot_memory::GblBootMemoryProtocol,
        gbl_efi_fastboot_transport::GblFastbootTransportProtocol,
    },
    EfiAllocator,
};
use efi_types::{
    protocol::gbl_efi_boot_memory::GblEfiBootMemoryManaged, EfiHandle, EfiSystemTable,
};
use efi_virtio_vsock::{gbl_vsock_init, FastbootTransport};
use gbl_efi_boot_control::GblEfiBootControlImpl;
use gbl_efi_boot_memory::GblEfiBootMemoryImpl;

#[unsafe(no_mangle)]
#[global_allocator]
static mut EFI_GLOBAL_ALLOCATOR: EfiAllocator = EfiAllocator::new();

/// Pull in the sysdeps required by libavb so the linker can find them.
extern crate avb_sysdeps;
/// Pull in the sysdeps required by boringssl so the linker can find them.
extern crate boringssl_sysdeps;
/// Pull in the getentropy implementation.
extern crate efi_rng;

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
    efi_println!(entry, "Loaded FDT with {} bytes", read_size);

    let bs = entry.system_table().boot_services();

    static GBL_EFI_BOOT_MEMORY_MANAGED: GblEfiBootMemoryManaged<GblEfiBootMemoryImpl> =
        GblEfiBootMemoryManaged::new(GblEfiBootMemoryImpl);
    bs.install_protocol_from_rust::<GblBootMemoryProtocol, _>(None, &GBL_EFI_BOOT_MEMORY_MANAGED)
        .unwrap();

    static GBL_EFI_BOOT_CONTROL_IMPL: GblEfiBootControlImpl = GblEfiBootControlImpl;
    bs.install_protocol_from_rust::<GblBootControlProtocol, _>(None, &GBL_EFI_BOOT_CONTROL_IMPL)
        .unwrap();

    // Fastboot over vsock
    efi_println!(entry, "Initializing vsock {:?}", gbl_vsock_init(&entry));
    static FASTBOOT_VSOCK_TRANSPORT: [FastbootTransport; 2] = [
        FastbootTransport::new(1, c"Fastboot over vsock port 1"),
        FastbootTransport::new(2, c"Fastboot over vsock port 2"),
    ];
    efi_println!(entry, "Installing fastboot over vsock transports");
    for transport in FASTBOOT_VSOCK_TRANSPORT.iter() {
        transport.init().unwrap();
        bs.install_protocol_from_rust::<GblFastbootTransportProtocol, _>(None, transport).unwrap();
    }

    let mut gbl_file =
        semihosting::File::open(c"gbl.bin", semihosting::OpenMode::ReadBinary).unwrap();
    let mut gbl_buffer = alloc::vec![0u8; gbl_file.len().unwrap()];
    let gbl_read_size = gbl_file.read(&mut gbl_buffer).unwrap();
    efi_println!(entry, "Loaded GBL with {} bytes", gbl_read_size);

    // TODO(b/499359597): Mock EFI protocols and launch GBL.

    efi_println!(entry, "Starting GBL..");
    // SAFETY:
    // `gbl_buffer` contains a valid PE image.
    unsafe { entry.system_table().boot_services().load_and_start_image(&mut gbl_buffer).unwrap() };

    unreachable!();
}

#[panic_handler]
fn handle_panic(_p_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
