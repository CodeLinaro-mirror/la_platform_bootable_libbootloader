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

use crate::{fastboot::efi_gbl_fastboot_entry, ops::Ops};
use efi::{efi_println, EfiEntry};
use libgbl::{
    android_boot::{android_main, BootBuffer},
    gbl_println, GblOps, Result,
};

/// Android bootloader main entry (before booting).
///
/// On success, returns a tuple of slices (ramdisk, fdt, kernel, remains).
pub fn efi_android_load<'a>(
    ops: &mut Ops,
    load: BootBuffer<'a>,
) -> Result<(&'a [u8], &'a [u8], &'a [u8], &'a mut [u8])> {
    let entry = ops.efi_entry;
    gbl_println!(ops, "Try booting as Android");
    Ok(android_main(ops, load, |fb| efi_gbl_fastboot_entry(entry, fb))?)
}

/// Boots loaded android images.
#[cfg_attr(feature = "efi_boot_stub", allow(unused_variables))]
pub fn efi_android_boot(
    entry: EfiEntry,
    kernel: &[u8],
    ramdisk: &[u8],
    fdt: &[u8],
    remains: &mut [u8],
) -> Result<()> {
    efi_println!(entry, "");
    efi_println!(
        entry,
        "Booting kernel @ {:#x}, ramdisk @ {:#x}, fdt @ {:#x}",
        kernel.as_ptr() as usize,
        ramdisk.as_ptr() as usize,
        fdt.as_ptr() as usize
    );

    #[cfg(all(target_arch = "aarch64", feature = "gbl_tracing"))]
    {
        // TODO(b/473552136): Temporary test for tracing functionality.
        unsafe extern "C" {
            safe fn get_peak_stack() -> usize;
        }
        efi_println!(entry, "max stack used: {} bytes", get_peak_stack());
    }

    #[cfg(all(target_arch = "aarch64", not(feature = "efi_boot_stub")))]
    {
        let _ = efi::exit_boot_services(entry, remains)?;
        // SAFETY: We currently target at Cuttlefish emulator where images are provided valid.
        unsafe { boot::aarch64::jump_linux_el2_or_lower(kernel, ramdisk, fdt) };
    }

    #[cfg(all(target_arch = "x86_64", not(feature = "efi_boot_stub")))]
    {
        use fdt::Fdt;
        use liberror::Error;
        use libgbl::android_boot::device_tree::PROP_BOOTARGS;

        const EFI_PAGE_SIZE: u64 = efi_types::EFI_PAGE_SIZE as u64;

        let fdt = Fdt::new(&fdt[..])?;
        let systab_addr = entry.system_table().as_ptr() as usize;
        let efi_mmap = efi::exit_boot_services(entry, remains)?;
        // SAFETY: We currently target at Cuttlefish emulator where images are provided valid.
        unsafe {
            boot::x86::boot_linux_bzimage(
                kernel,
                ramdisk,
                fdt.get_property("chosen", PROP_BOOTARGS).unwrap(),
                |e820_entries, efi_info| {
                    // "EL64" Signature tells the kernel it's a 64-bit EFI boot
                    efi_info.efi_loader_signature = 0x34364c45;
                    // Pass the physical address of the UEFI System Table
                    efi_info.efi_systab = (systab_addr & 0xFFFFFFFF).try_into().unwrap();
                    efi_info.efi_systab_hi = (systab_addr >> 32).try_into().unwrap();

                    // Get the memory map.
                    let mmap_ptr = efi_mmap.buffer().as_ptr() as usize;

                    // Pass the Memory Map pointers and metadata
                    efi_info.efi_memdesc_version =
                        efi_mmap.descriptor_version().try_into().unwrap();
                    efi_info.efi_memdesc_size = efi_mmap.descriptor_size().try_into().unwrap();
                    efi_info.efi_memmap_size = efi_mmap.buffer().len().try_into().unwrap();
                    efi_info.efi_memmap = (mmap_ptr & 0xFFFFFFFF).try_into().unwrap();
                    efi_info.efi_memmap_hi = (mmap_ptr >> 32).try_into().unwrap();

                    let mut idx = 0;
                    for mem in efi_mmap.into_iter() {
                        let cur_type = crate::utils::efi_to_e820_mem_type(mem.memory_type);
                        // Coalesce adjacent memory regions of similar type.
                        // We intentionally check only against the previous entry (`idx - 1`) based on
                        // the expectation that the EFI memory map is mostly sorted. This strikes a
                        // balance between time efficiency and compaction.
                        if idx != 0
                            && (e820_entries[idx - 1].type_ == cur_type)
                            && ((e820_entries[idx - 1].addr + e820_entries[idx - 1].size)
                                == mem.physical_start)
                        {
                            e820_entries[idx - 1].size += mem.number_of_pages * 4096;
                        } else {
                            if idx >= e820_entries.len() {
                                return Err(Error::MemoryMapCallbackError(-1));
                            }

                            e820_entries[idx] = boot::x86::e820entry {
                                addr: mem.physical_start,
                                size: mem.number_of_pages * EFI_PAGE_SIZE,
                                type_: cur_type,
                            };
                            idx += 1;
                        }
                    }

                    // Sort the memory map entries by address as required by linux boot protocol.
                    e820_entries[0..idx].sort_unstable_by_key(|e820_entry| e820_entry.addr);

                    Ok(idx.try_into().unwrap())
                },
                0x9_0000,
            )?;
        }
    }

    #[cfg(target_arch = "riscv64")]
    {
        let boot_hart_id = entry
            .system_table()
            .boot_services()
            .find_first_and_open::<efi::protocol::riscv::RiscvBootProtocol>()?
            .get_boot_hartid()?;
        efi_println!(entry, "riscv boot_hart_id: {}", boot_hart_id);
        let _ = efi::exit_boot_services(entry, remains)?;
        // SAFETY: We currently target at Cuttlefish emulator where images are provided valid.
        unsafe { boot::riscv64::jump_linux(kernel, boot_hart_id, fdt) };
    }

    #[cfg(all(target_arch = "aarch64", feature = "efi_boot_stub"))]
    {
        use libgbl::android_boot::device_tree::PROP_BOOTARGS;

        /// Maximum length of kernel command line in bytes, matching Linux `COMMAND_LINE_SIZE`.
        const COMMAND_LINE_SIZE: usize = 2048;

        efi_println!(entry, "Loading PE/COFF kernel image");

        let bs = entry.system_table().boot_services();

        // SAFETY: `fdt` points to memory within `boot_buffer`. Because this function transfers
        // execution directly to the kernel and never returns from the kernel, `boot_buffer` is
        // never dropped nor deallocated during EFI execution.
        unsafe {
            bs.install_configuration_table(&efi::EFI_DTB_TABLE_GUID, fdt.as_ptr() as *mut _)?;
        }

        // SAFETY: `kernel` points to valid kernel image bytes.
        let mut loaded_image = unsafe { bs.load_image(kernel)? };

        // Convert FDT bootargs to UTF-16 and install into LoadOptions.
        let options: arrayvec::ArrayVec<u16, COMMAND_LINE_SIZE> =
            if let Ok(bootargs) = fdt::Fdt::new(fdt)?.get_property("chosen", PROP_BOOTARGS) {
                bootargs
                    .iter()
                    .copied()
                    .take_while(|&b| b != 0)
                    .map(|b| b as u16)
                    .chain(core::iter::once(0))
                    .take(COMMAND_LINE_SIZE)
                    .collect()
            } else {
                arrayvec::ArrayVec::new()
            };

        if !options.is_empty() {
            // SAFETY: `options` is stack-allocated on the current function frame and outlives
            // `loaded_image.start()`.
            unsafe {
                loaded_image.protocol.set_load_options(
                    options.as_ptr() as *const _,
                    (options.len() * core::mem::size_of::<u16>()) as u32,
                );
            }
        }

        // Install the fixed placement protocol to disable physical KASLR (does NOT disable virtual
        // KASLR which is controlled via kaslr-seed FDT node).
        bs.install_null_protocol_interface(
            &mut loaded_image.image_handle,
            &efi::LINUX_EFI_LOADED_IMAGE_FIXED_GUID,
        )?;

        efi_println!(entry, "Starting kernel EFI boot stub");
        loaded_image.start()?;
        unreachable!();
    }

    #[cfg(all(target_arch = "x86_64", feature = "efi_boot_stub"))]
    {
        use libgbl::android_boot::device_tree::PROP_BOOTARGS;

        let fdt = fdt::Fdt::new(&fdt[..])?;
        let cmdline = fdt.get_property("chosen", PROP_BOOTARGS).unwrap();
        // TODO(b/477970734): The EFI handover protocol is deprecated per
        // https://www.kernel.org/doc/html/v6.6/arch/x86/boot.html#efi-handover-protocol-deprecated
        // We should shift to the standard BS.load_image() && .start_image() in the future.

        // Mark the boot sector (loaded at LOW_MEMORY_ADDR) and kernel image (loaded at
        // HIGH_MEMORY_ADDR) as used, so that BOOT_SERVICES.allocate_pages() don't allocate these
        // regions for others to use.
        const EFI_PAGE_SIZE: usize = efi_types::EFI_PAGE_SIZE as _;
        const LOW_MEMORY_ADDR: u64 = 0x9_0000;
        const HIGH_MEMORY_ADDR: u64 = 0x10_0000;
        // Boot sector must fit within [0x9_0000, 0xA_0000]
        const BOOT_SECTOR_MAX_SIZE: usize = 0x1_0000;
        let _ = entry.system_table().boot_services().allocate_pages(
            efi_types::EFI_MEMORY_TYPE_LOADER_DATA,
            efi::AllocationAddress::Fixed(LOW_MEMORY_ADDR),
            BOOT_SECTOR_MAX_SIZE / EFI_PAGE_SIZE,
        )?;
        let _ = entry.system_table().boot_services().allocate_pages(
            efi_types::EFI_MEMORY_TYPE_LOADER_DATA,
            efi::AllocationAddress::Fixed(HIGH_MEMORY_ADDR),
            (kernel.len() + EFI_PAGE_SIZE - 1) / EFI_PAGE_SIZE,
        )?;

        // SAFETY: We currently target at Cuttlefish emulator where images are provided valid.
        unsafe {
            boot::x86::boot_linux_bzimage_efi_handover(
                kernel,
                ramdisk,
                cmdline,
                LOW_MEMORY_ADDR as _,
                entry.image_handle_ptr(),
                entry.system_table_ptr(),
            )?;
        }
    }
}
