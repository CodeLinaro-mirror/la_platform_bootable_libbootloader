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

use crate::{efi, ops::get_buffer_from_protocol};
use ::efi::{efi_println, EfiMemoryAttributesTable};
use core::{slice::from_raw_parts_mut, str::from_utf8, time::Duration};
use efi::{
    protocol::{
        device_path::{DevicePathProtocol, DevicePathText, DevicePathToTextProtocol},
        gbl_efi_boot_memory::{
            gbl_clear_boot_buffer, gbl_get_boot_buffer, GblVendorReservedMemory,
        },
        gbl_efi_image_loading::EfiImageBufferInfo,
        loaded_image::LoadedImageProtocol,
        simple_text_input::SimpleTextInputProtocol,
    },
    utils::Timeout,
    DeviceHandle, EfiEntry,
};
use efi_types::{
    EfiGuid, EfiInputKey, EfiMemoryType, GblEfiBootBufferType,
    GBL_EFI_BOOT_BUFFER_TYPE_FASTBOOT_DOWNLOAD, GBL_EFI_BOOT_BUFFER_TYPE_FDT,
    GBL_EFI_BOOT_BUFFER_TYPE_GENERAL_LOAD, GBL_EFI_BOOT_BUFFER_TYPE_KERNEL,
    GBL_EFI_BOOT_BUFFER_TYPE_PVMFW_DATA, GBL_EFI_BOOT_BUFFER_TYPE_RAMDISK, GBL_IMAGE_TYPE_FASTBOOT,
    GBL_IMAGE_TYPE_OS_LOAD, GBL_IMAGE_TYPE_PVMFW_DATA,
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
pub fn loaded_image_path(entry: &EfiEntry) -> Result<DevicePathText> {
    get_device_path(
        entry,
        entry
            .system_table()
            .boot_services()
            .open_protocol::<LoadedImageProtocol>(entry.image_handle())?
            .device_handle(),
    )
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

/// Represents either an initialized static memory space or memory to be allocated by the given
/// size.
pub(crate) enum BufferInfo {
    // A static memory space, i.e. memory space reserved by platform
    Static(&'static mut [u8]),
    Alloc(usize),
}

/// A helper for getting platform buffer info from EFI image loading protocol.
pub(crate) fn get_platform_buffer_info(
    efi_entry: &EfiEntry,
    image_type: &str,
    default_aloc_size: usize,
) -> BufferInfo {
    match get_buffer_from_protocol(efi_entry, image_type, 0) {
        Ok(EfiImageBufferInfo::Buffer(mut buffer)) => {
            let buffer = buffer.take();
            buffer.fill(core::mem::MaybeUninit::zeroed());
            efi_println!(
                efi_entry,
                "Found \"{image_type}\" buffer from EFI protocol: addr {:#x}, sz: {:#x}.",
                buffer.as_mut_ptr() as usize,
                buffer.len()
            );
            // SAFETY:
            // * `buffer` is a &'static [MaybeUninit<u8>] and fully initialized by the previous
            //   line.
            // * MaybeUninit::zeroed() is a valid initialized value for u8.
            BufferInfo::Static(unsafe {
                from_raw_parts_mut(buffer.as_mut_ptr() as _, buffer.len())
            })
        }
        Ok(EfiImageBufferInfo::AllocSize(sz)) if sz != 0 => BufferInfo::Alloc(sz),
        _ => BufferInfo::Alloc(default_aloc_size),
    }
}

pub(crate) const SZ_MB: usize = 1024 * 1024;

/// Represents a buffer from either GblEfiImageLoading protocol or GblEfiBootMemory protocol.
// TODO(b/430068343): Switch to GblEfiBootMemory entirely.
enum VendorReservedMemory {
    /// From GblEfiImageLoading protocol.
    Legacy(&'static mut [u8]),
    /// From GblEfiBootMemory protocol.
    Buffer(GblVendorReservedMemory),
}

impl VendorReservedMemory {
    /// Gets the buffer
    fn get(&mut self) -> &mut [u8] {
        match self {
            Self::Legacy(ref mut v) => v,
            Self::Buffer(ref mut v) => v,
        }
    }
}

/// Finds boot buffer from GblEfiBootMemoryProtocol or GlbEfiImageLoadingProtocol.
fn get_boot_buffer_check_legacy(
    entry: &EfiEntry,
    buffer_type: GblEfiBootBufferType,
    legacy_type: &str,
    default: usize,
) -> Result<Option<VendorReservedMemory>> {
    // Check if platform is using legacy GblEfiImageLoading protocol.
    let res = match get_platform_buffer_info(&entry, legacy_type, 0) {
        BufferInfo::Static(v) => Some(v),
        BufferInfo::Alloc(sz) if sz != 0 => {
            let alloc = vec![0u8; sz];
            efi_println!(entry, "Allocated {sz:#x} bytes for {legacy_type:?} buffer.");
            Some(alloc.leak())
        }
        _ => None,
    };

    Ok(match res {
        Some(v) => {
            efi_println!(entry, "Warning: GblEfiImageLoading protocol is being deprecated");
            efi_println!(entry, "Please migrate to GblEfiBootMemory protocol");
            Some(VendorReservedMemory::Legacy(v))
        }
        _ => match gbl_get_boot_buffer(entry, buffer_type, default) {
            Err(Error::NotFound) => None,
            v => Some(VendorReservedMemory::Buffer(v?)),
        },
    })
}

/// Intermediate strucutre that can generate a `BootBuffer` instance.
pub(crate) struct GblEfiBootBuffer {
    general: VendorReservedMemory,
    kernel: Option<GblVendorReservedMemory>,
    ramdisk: Option<GblVendorReservedMemory>,
    fdt: Option<GblVendorReservedMemory>,
    // TODO(b/430068343): Switch to Option<GblVendorReservedMemory> from GblEfiBootMemoryProtocol.
    pvmfw_data: Option<VendorReservedMemory>,
}

impl GblEfiBootBuffer {
    pub(crate) fn to_boot_buffer(&mut self) -> BootBuffer<'_> {
        BootBuffer {
            general: self.general.get(),
            kernel: self.kernel.as_mut().map(|v| v as _),
            ramdisk: self.ramdisk.as_mut().map(|v| v as _),
            fdt: self.fdt.as_mut().map(|v| v as _),
            pvmfw_data: self.pvmfw_data.as_mut().map(|v| v.get()),
        }
    }
}

/// Helper for getting boot buffer.
pub(crate) fn get_boot_buffer(entry: &EfiEntry, default: usize) -> Result<GblEfiBootBuffer> {
    let [kernel, ramdisk, fdt] = [
        GBL_EFI_BOOT_BUFFER_TYPE_KERNEL,
        GBL_EFI_BOOT_BUFFER_TYPE_RAMDISK,
        GBL_EFI_BOOT_BUFFER_TYPE_FDT,
    ]
    .map(|v| match gbl_get_boot_buffer(entry, v, 0) {
        Err(Error::NotFound) => Ok(None),
        v => v.map(|v| Some(v)),
    });

    let general = get_boot_buffer_check_legacy(
        entry,
        GBL_EFI_BOOT_BUFFER_TYPE_GENERAL_LOAD,
        from_utf8(GBL_IMAGE_TYPE_OS_LOAD).unwrap(),
        default,
    )?
    .unwrap();
    let pvmfw_data = get_boot_buffer_check_legacy(
        entry,
        GBL_EFI_BOOT_BUFFER_TYPE_PVMFW_DATA,
        from_utf8(GBL_IMAGE_TYPE_PVMFW_DATA).unwrap(),
        0,
    )?;
    Ok(GblEfiBootBuffer { general, kernel: kernel?, ramdisk: ramdisk?, fdt: fdt?, pvmfw_data })
}

/// Represents a fastboot buffer
pub(crate) struct FastbootBuffer<'a> {
    entry: &'a EfiEntry,
    // Uses option so that we can manually drop it.
    buffer: Option<VendorReservedMemory>,
}

impl<'a> FastbootBuffer<'a> {
    /// Requests the buffer.
    pub(crate) fn new(entry: &'a EfiEntry) -> Result<Self> {
        let buffer = Some(
            get_boot_buffer_check_legacy(
                entry,
                GBL_EFI_BOOT_BUFFER_TYPE_FASTBOOT_DOWNLOAD,
                from_utf8(GBL_IMAGE_TYPE_FASTBOOT).unwrap(),
                512 * SZ_MB,
            )?
            .unwrap(),
        );
        Ok(FastbootBuffer { entry, buffer })
    }

    /// Gets the buffer
    pub(crate) fn get(&mut self) -> &mut [u8] {
        self.buffer.as_mut().unwrap().get()
    }
}

impl Drop for FastbootBuffer<'_> {
    fn drop(&mut self) {
        if let VendorReservedMemory::Legacy(_) = self.buffer.take().unwrap() {
            return;
        }
        gbl_clear_boot_buffer(self.entry, GBL_EFI_BOOT_BUFFER_TYPE_FASTBOOT_DOWNLOAD).unwrap();
    }
}
