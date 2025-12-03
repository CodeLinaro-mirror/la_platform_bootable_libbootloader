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

//! Contains util APIs for EFI.

use crate::efi;
use ::efi::{efi_println, EfiMemoryAttributesTable};
use core::time::Duration;
use efi::{
    protocol::{
        device_path::{DevicePathProtocol, DevicePathText, DevicePathToTextProtocol},
        gbl_efi_boot_memory::{
            gbl_clear_boot_buffer, gbl_get_boot_buffer, GblVendorReservedMemory,
        },
        loaded_image::LoadedImageProtocol,
        simple_text_input::SimpleTextInputProtocol,
    },
    utils::Timeout,
    DeviceHandle, EfiEntry,
};
#[cfg(any(target_arch = "x86_64", feature = "fuchsia"))]
use efi_types::EfiMemoryType;
use efi_types::{
    EfiGuid, EfiInputKey, GBL_EFI_BOOT_BUFFER_TYPE_FASTBOOT_DOWNLOAD, GBL_EFI_BOOT_BUFFER_TYPE_FDT,
    GBL_EFI_BOOT_BUFFER_TYPE_GENERAL_LOAD, GBL_EFI_BOOT_BUFFER_TYPE_KERNEL,
    GBL_EFI_BOOT_BUFFER_TYPE_PVMFW_DATA, GBL_EFI_BOOT_BUFFER_TYPE_RAMDISK,
};
use fdt::FdtHeader;
use liberror::Error;
use libgbl::android_boot::BootBuffer;

type Result<T> = core::result::Result<T, Error>;

pub(crate) const EFI_DTB_TABLE_GUID: EfiGuid =
    EfiGuid::new(0xb1b621d5, 0xf19c, 0x41a5, [0x83, 0x0b, 0xd9, 0x15, 0x2c, 0x69, 0xaa, 0xe0]);

/// Helper function to get the `DevicePathText` from a `DeviceHandle`.
pub fn get_device_path<'a>(
    entry: &'a EfiEntry,
    handle: DeviceHandle,
) -> Result<DevicePathText<'a>> {
    let bs = entry.system_table().boot_services();
    let path = bs.open_protocol::<DevicePathProtocol>(handle)?;
    let path_to_text = bs.find_first_and_open::<DevicePathToTextProtocol>()?;
    Ok(path_to_text.convert_device_path_to_text(&path, false, false)?)
}

/// Helper function to get the loaded image path.
pub fn loaded_image_path(entry: &EfiEntry) -> Result<DevicePathText<'_>> {
    let bs = entry.system_table().boot_services();
    let path_to_text = bs.find_first_and_open::<DevicePathToTextProtocol>()?;
    let loaded_image = bs.open_protocol::<LoadedImageProtocol>(entry.image_handle())?;
    if let Ok(file_path) = loaded_image.file_path() {
        path_to_text.convert_device_path_to_text(&file_path, false, false)
    } else {
        let device_path = bs.open_protocol::<DevicePathProtocol>(loaded_image.device_handle())?;
        path_to_text.convert_device_path_to_text(&device_path, false, false)
    }
}

/// Helper function to get the loaded image base address.
pub fn image_base(entry: &EfiEntry) -> Result<usize> {
    Ok(entry
        .system_table()
        .boot_services()
        .open_protocol::<LoadedImageProtocol>(entry.image_handle())
        .inspect_err(|e| efi_println!(entry, "Failed to open LoadedImageProtocol: {e}"))?
        .image_base())
}

/// Find FDT from EFI configuration table.
pub fn get_efi_fdt(entry: &EfiEntry) -> Option<(&FdtHeader, &[u8])> {
    if let Some(config_tables) = entry.system_table().configuration_table() {
        for table in config_tables {
            if table.vendor_guid == EFI_DTB_TABLE_GUID {
                // SAFETY: Buffer provided by EFI configuration table.
                return unsafe { FdtHeader::from_raw(table.vendor_table as *const _).ok() };
            }
        }
    }
    None
}

#[cfg(any(target_arch = "x86_64"))]
pub(crate) fn efi_to_e820_mem_type(efi_mem_type: EfiMemoryType) -> u32 {
    match efi_mem_type {
        efi_types::EFI_MEMORY_TYPE_LOADER_CODE
        | efi_types::EFI_MEMORY_TYPE_LOADER_DATA
        | efi_types::EFI_MEMORY_TYPE_BOOT_SERVICES_CODE
        | efi_types::EFI_MEMORY_TYPE_BOOT_SERVICES_DATA
        | efi_types::EFI_MEMORY_TYPE_CONVENTIONAL_MEMORY => boot::x86::E820_ADDRESS_TYPE_RAM,
        efi_types::EFI_MEMORY_TYPE_RUNTIME_SERVICES_CODE
        | efi_types::EFI_MEMORY_TYPE_RUNTIME_SERVICES_DATA
        | efi_types::EFI_MEMORY_TYPE_MEMORY_MAPPED_IO
        | efi_types::EFI_MEMORY_TYPE_MEMORY_MAPPED_IOPORT_SPACE
        | efi_types::EFI_MEMORY_TYPE_PAL_CODE
        | efi_types::EFI_MEMORY_TYPE_RESERVED_MEMORY_TYPE => boot::x86::E820_ADDRESS_TYPE_RESERVED,
        efi_types::EFI_MEMORY_TYPE_UNUSABLE_MEMORY => boot::x86::E820_ADDRESS_TYPE_UNUSABLE,
        efi_types::EFI_MEMORY_TYPE_ACPIRECLAIM_MEMORY => boot::x86::E820_ADDRESS_TYPE_ACPI,
        efi_types::EFI_MEMORY_TYPE_ACPIMEMORY_NVS => boot::x86::E820_ADDRESS_TYPE_NVS,
        efi_types::EFI_MEMORY_TYPE_PERSISTENT_MEMORY => boot::x86::E820_ADDRESS_TYPE_PMEM,
        v => panic!("Unmapped EFI memory type {:?}", v),
    }
}

/// Repetitively runs a closure until it signals completion or timeout.
///
/// * If `f` returns `Ok(R)`, an `Ok(Some(R))` is returned immediately.
/// * If `f` has been repetitively called and returning `Err(false)` for `timeout_duration`,  an
///   `Ok(None)` is returned. This is the time out case.
/// * If `f` returns `Err(true)` the timeout is reset.
pub fn loop_with_timeout<F, R>(
    efi_entry: &EfiEntry,
    timeout_duration: Duration,
    mut f: F,
) -> Result<Option<R>>
where
    F: FnMut() -> core::result::Result<R, bool>,
{
    let timeout = Timeout::new(efi_entry, timeout_duration)?;
    while !timeout.check()? {
        match f() {
            Ok(v) => return Ok(Some(v)),
            Err(true) => timeout.reset(timeout_duration)?,
            _ => {}
        }
    }
    Ok(None)
}

/// Waits for a key stroke value from simple text input.
///
/// Returns `Ok(true)` if the expected key stroke is read, `Ok(false)` if timeout, `Err` otherwise.
pub fn wait_key_stroke(
    efi_entry: &EfiEntry,
    pred: impl Fn(EfiInputKey) -> bool,
    timeout: Duration,
) -> Result<bool> {
    let input = efi_entry
        .system_table()
        .boot_services()
        .find_first_and_open::<SimpleTextInputProtocol>()?;
    loop_with_timeout(efi_entry, timeout, || -> core::result::Result<Result<bool>, bool> {
        match input.read_key_stroke() {
            Ok(Some(key)) if pred(key) => Ok(Ok(true)),
            Err(e) => Ok(Err(e.into())),
            _ => Err(false),
        }
    })?
    .unwrap_or(Ok(false))
}

// Converts an EFI memory type to a zbi_mem_range_t type.
#[cfg(feature = "fuchsia")]
pub(crate) fn efi_to_zbi_mem_range_type(efi_mem_type: EfiMemoryType) -> u32 {
    match efi_mem_type {
        efi_types::EFI_MEMORY_TYPE_LOADER_CODE
        | efi_types::EFI_MEMORY_TYPE_LOADER_DATA
        | efi_types::EFI_MEMORY_TYPE_BOOT_SERVICES_CODE
        | efi_types::EFI_MEMORY_TYPE_BOOT_SERVICES_DATA
        | efi_types::EFI_MEMORY_TYPE_CONVENTIONAL_MEMORY => zbi::zbi_format::ZBI_MEM_TYPE_RAM,
        _ => zbi::zbi_format::ZBI_MEM_TYPE_RESERVED,
    }
}

/// Find Memory attributes from EFI configuration_table
#[allow(unused)]
pub(crate) fn get_efi_mem_attr<'a>(
    entry: &'a EfiEntry,
) -> Option<EfiMemoryAttributesTable<'static>> {
    entry.system_table().configuration_table().and_then(|config_tables| {
        config_tables
            .iter()
            .find_map(|&table| {
                // SAFETY:
                // `table` is valid EFI Configuration table provided by EFI
                match unsafe { EfiMemoryAttributesTable::new(table) } {
                    Err(Error::NotFound) => None,
                    other => Some(other.ok()),
                }
            })
            .flatten()
    })
}

pub(crate) const SZ_MB: usize = 1024 * 1024;

/// Intermediate strucutre that can generate a `BootBuffer` instance.
pub(crate) struct GblEfiBootBuffer {
    general: GblVendorReservedMemory,
    kernel: Option<GblVendorReservedMemory>,
    ramdisk: Option<GblVendorReservedMemory>,
    fdt: Option<GblVendorReservedMemory>,
    pvmfw_data: Option<GblVendorReservedMemory>,
}

impl GblEfiBootBuffer {
    pub(crate) fn to_boot_buffer(&mut self) -> BootBuffer<'_> {
        BootBuffer::new(
            &mut self.general,
            self.kernel.as_mut().map(|v| v as _),
            self.ramdisk.as_mut().map(|v| v as _),
            self.fdt.as_mut().map(|v| v as _),
            self.pvmfw_data.as_mut().map(|v| v as _),
        )
    }
}

/// Helper for getting boot buffer.
pub(crate) fn get_boot_buffer(entry: &EfiEntry, default: usize) -> Result<GblEfiBootBuffer> {
    let [kernel, ramdisk, fdt, pvmfw_data] = [
        GBL_EFI_BOOT_BUFFER_TYPE_KERNEL,
        GBL_EFI_BOOT_BUFFER_TYPE_RAMDISK,
        GBL_EFI_BOOT_BUFFER_TYPE_FDT,
        GBL_EFI_BOOT_BUFFER_TYPE_PVMFW_DATA,
    ]
    .map(|v| match gbl_get_boot_buffer(entry, v, 0) {
        Err(Error::NotFound) => Ok(None),
        v => v.map(|v| Some(v)),
    });

    let general = gbl_get_boot_buffer(entry, GBL_EFI_BOOT_BUFFER_TYPE_GENERAL_LOAD, default)?;
    Ok(GblEfiBootBuffer {
        general,
        kernel: kernel?,
        ramdisk: ramdisk?,
        fdt: fdt?,
        pvmfw_data: pvmfw_data?,
    })
}

/// Represents a fastboot buffer
pub(crate) struct FastbootBuffer<'a> {
    entry: &'a EfiEntry,
    // Uses option so that we can manually drop it.
    buffer: Option<GblVendorReservedMemory>,
}

impl<'a> FastbootBuffer<'a> {
    /// Requests the buffer.
    pub(crate) fn new(entry: &'a EfiEntry) -> Result<Self> {
        let buffer = Some(gbl_get_boot_buffer(
            entry,
            GBL_EFI_BOOT_BUFFER_TYPE_FASTBOOT_DOWNLOAD,
            512 * SZ_MB,
        )?);
        Ok(FastbootBuffer { entry, buffer })
    }

    /// Gets the buffer
    pub(crate) fn get(&mut self) -> &mut [u8] {
        self.buffer.as_mut().unwrap()
    }
}

impl Drop for FastbootBuffer<'_> {
    fn drop(&mut self) {
        gbl_clear_boot_buffer(self.entry, GBL_EFI_BOOT_BUFFER_TYPE_FASTBOOT_DOWNLOAD).unwrap();
    }
}
