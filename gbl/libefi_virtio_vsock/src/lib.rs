// Copyright (C) 2026 The Android Open Source Project
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
// limitations under the License.

//! This crate provides utilities for Virtio VSOCK in GBL.

#![no_std]

extern crate alloc;

use acpi::{sdt::mcfg::Mcfg, AcpiTables, Handler as AcpiHandler, PciAddress, PhysicalMapping};
use alloc::alloc::{alloc, dealloc, Layout};
use core::ptr::NonNull;
use efi::{
    utils::{find_acpi_configuration_table, find_fdt_configuration_table},
    EfiEntry,
};
use liberror::Error;
use spin::Mutex;
pub use virtio_drivers::device::socket::VsockAddr;
use virtio_drivers::{
    device::socket::{VirtIOSocket, VsockConnectionManager},
    transport::{
        pci::{
            bus::{Cam, MmioCam, PciRoot},
            virtio_device_type, PciTransport,
        },
        DeviceType,
    },
    BufferDirection, Hal, PhysAddr,
};

// TODO(b/486979232): Move this constant to the EFI crate and unifies all similar constants.
const EFI_PAGE_SIZE: usize = 4096;

#[derive(Clone)]
struct GblAcpiHandler;

#[rustfmt::skip]
impl AcpiHandler for GblAcpiHandler {
    /// # Safety: Caller should follow the safety requirements in
    /// acpi::Handler::map_physical_region.
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        PhysicalMapping {
            physical_start: physical_address,
            virtual_start: NonNull::new(physical_address as _).unwrap(),
            region_length: size,
            mapped_length: size,
            handler: GblAcpiHandler,
        }
    }

    fn unmap_physical_region<T>(_: &PhysicalMapping<Self, T>) {}
    fn read_u8(&self, _address: usize) -> u8 { unimplemented!() }
    fn read_u16(&self, _address: usize) -> u16 { unimplemented!() }
    fn read_u32(&self, _address: usize) -> u32 { unimplemented!() }
    fn read_u64(&self, _address: usize) -> u64 { unimplemented!() }
    fn write_u8(&self, _address: usize, _value: u8) { unimplemented!() }
    fn write_u16(&self, _address: usize, _value: u16) { unimplemented!() }
    fn write_u32(&self, _address: usize, _value: u32) { unimplemented!() }
    fn write_u64(&self, _address: usize, _value: u64) { unimplemented!() }
    fn read_io_u8(&self, _port: u16) -> u8 { unimplemented!() }
    fn read_io_u16(&self, _port: u16) -> u16 { unimplemented!() }
    fn read_io_u32(&self, _port: u16) -> u32 { unimplemented!() }
    fn write_io_u8(&self, _port: u16, _value: u8) { unimplemented!() }
    fn write_io_u16(&self, _port: u16, _value: u16) { unimplemented!() }
    fn write_io_u32(&self, _port: u16, _value: u32) { unimplemented!() }
    fn read_pci_u8(&self, _address: PciAddress, _offset: u16) -> u8 { unimplemented!() }
    fn read_pci_u16(&self, _address: PciAddress, _offset: u16) -> u16 { unimplemented!() }
    fn read_pci_u32(&self, _address: PciAddress, _offset: u16) -> u32 { unimplemented!() }
    fn write_pci_u8(&self, _address: PciAddress, _offset: u16, _value: u8) { unimplemented!() }
    fn write_pci_u16(&self, _address: PciAddress, _offset: u16, _value: u16) { unimplemented!() }
    fn write_pci_u32(&self, _address: PciAddress, _offset: u16, _value: u32) { unimplemented!() }
    fn nanos_since_boot(&self) -> u64 { unimplemented!() }
    fn stall(&self, _microseconds: u64) { unimplemented!() }
    fn sleep(&self, _milliseconds: u64) { unimplemented!() }
}

/// Detects the ECAM base address using ACPI or Device Tree.
pub fn detect_ecam(entry: &EfiEntry) -> Option<(u64, u64)> {
    get_ecam_from_acpi(entry).or_else(|| get_ecam_from_dt(entry))
}

/// Gets ECAM address and size from ACPI (i.e. EDK platforms).
fn get_ecam_from_acpi(entry: &EfiEntry) -> Option<(u64, u64)> {
    let acpi_ptr = find_acpi_configuration_table(entry)?;
    // SAFETY: By UEFI spec, the ACPI configuration table gives the ACPI address.
    let tables = unsafe { AcpiTables::from_rsdp(GblAcpiHandler, acpi_ptr as _).ok()? };

    let mcfg_mapping = tables.find_table::<Mcfg>()?;
    mcfg_mapping.entries().iter().next().map(|entry| {
        (
            entry.base_address,
            (entry.bus_number_end as u64 - entry.bus_number_start as u64 + 1) * 0x100000,
        )
    })
}

/// Gets ECAM address and size from device tree (i.e. u-boot platforms).
fn get_ecam_from_dt(entry: &EfiEntry) -> Option<(u64, u64)> {
    let (_header, dt) = find_fdt_configuration_table(entry)?;
    let fdt = fdt::Fdt::new(dt).ok()?;
    // Virtio is almost exclusively used for qemu, which hardcodes pcie@10000000 as the base
    // address for pcie devices. Revisit if we ever use this for non-qemu platforms.
    let pcie_prop = fdt.get_property("pcie@10000000", c"reg").ok()?;
    let pcie_base = u64::from_be_bytes(pcie_prop[..8].try_into().unwrap());
    let pci_size = u64::from_be_bytes(pcie_prop[8..][..8].try_into().unwrap());
    Some((pcie_base, pci_size))
}

/// HAL implementation for VirtIO in GBL.
pub struct GblVirtIoHal {}

/// SAFETY:
/// The implementation meets the safety requirements by `virtio_drivers::Hal`:
/// * `dma_alloc` guarantees to return valid, page-aligned, zeroed memory. The memory is not
///   aliased by any other allocation or references.
/// * `mmio_phys_to_virt` returns the same pointer as the input physical address. Because GBL
///   requires identity mapping, the return pointer is guaranteed to be valid.
unsafe impl Hal for GblVirtIoHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        // SAFETY: The layout is valid and the allocation will be deallocated by dma_dealloc.
        let ptr = NonNull::new(unsafe {
            alloc(Layout::from_size_align(pages * EFI_PAGE_SIZE, EFI_PAGE_SIZE).unwrap())
        })
        .unwrap();
        // SAFETY: The pointer is non-null and points to a valid allocated region.
        unsafe { ptr.write_bytes(0, pages * EFI_PAGE_SIZE) };
        (ptr.as_ptr() as _, ptr)
    }

    /// # Safety: Caller should follow the safety requirements in
    /// virtio_drivers::Hal::dma_dealloc.
    unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, pages: usize) -> i32 {
        // SAFETY: The memory points to a valid allocated region allocated by dma_alloc.
        unsafe {
            dealloc(
                paddr as _,
                Layout::from_size_align(pages * EFI_PAGE_SIZE, EFI_PAGE_SIZE).unwrap(),
            )
        };
        0
    }

    /// # Safety: Caller should follow the safety requirements in
    /// virtio_drivers::Hal::mmio_phys_to_virt.
    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        // NA. GBL operates on physical or identity map address space.
        NonNull::new(paddr as _).unwrap()
    }

    /// # Safety: Caller should follow the safety requirements in
    /// virtio_drivers::Hal::share.
    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        // NA. GBL operates on physical or identity map address space.
        buffer.as_ptr().cast::<usize>() as _
    }

    /// # Safety: Caller should follow the safety requirements in
    /// virtio_drivers::Hal::unshare.
    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {
        // NA. GBL operates on physical or identity map address space.
    }
}

// Recommended buffer size for virtio-vsock.
const MAX_BUFFER_SIZE: usize = 64 * 1024;

/// GBL VirtIO VSOCK Connection Manager.
struct GblVsockConnectionManager {
    _connection_manager: VsockConnectionManager<GblVirtIoHal, PciTransport, MAX_BUFFER_SIZE>,
}

impl GblVsockConnectionManager {
    /// Creates a new instance.
    pub fn new(
        connection_manager: VsockConnectionManager<GblVirtIoHal, PciTransport, MAX_BUFFER_SIZE>,
    ) -> Self {
        Self { _connection_manager: connection_manager }
    }

    // TODO(b/486979232): Implement the vsock manager APIs.
}

/// Represents the global vsock manager.
struct GblVsockInitState(Option<Result<GblVsockConnectionManager, Error>>);

static GBL_VSOCK_MANAGER: Mutex<GblVsockInitState> = Mutex::new(GblVsockInitState(None));

/// Initializes the vsock manager.
pub fn gbl_vsock_init(entry: &EfiEntry) -> Result<(), Error> {
    GBL_VSOCK_MANAGER
        .try_lock()
        .ok_or(Error::NotReady)?
        .0
        .get_or_insert_with(|| {
            let (pci_base, pci_size) = detect_ecam(entry).ok_or(Error::Unsupported)?;
            // SAFETY: pci_base is from ACPI or Device Tree and gives the correct PCI address.
            let pci_root = &mut PciRoot::new(unsafe { MmioCam::new(pci_base as _, Cam::Ecam) });
            // Iterate over all possible PCI buses within the discovered ECAM space (which allocates
            // 1MB/0x100000 per bus) to enumerate devices and find the first VirtIO VSOCK device.
            for bus in u8::MIN..=u8::MAX {
                if bus as u64 * 0x100000 >= pci_size {
                    break;
                }

                let Some((func, _info)) = pci_root
                    .enumerate_bus(bus)
                    .find(|(_, info)| virtio_device_type(info) == Some(DeviceType::Socket))
                else {
                    continue;
                };
                let transport = PciTransport::new::<GblVirtIoHal, _>(pci_root, func)
                    .map_err(|_| Error::Other(Some("Failed to create PCI transport")))?;
                let vsock = VirtIOSocket::<GblVirtIoHal, _, MAX_BUFFER_SIZE>::new(transport)
                    .map_err(|_| Error::Other(Some("Failed to create vsock")))?;
                return Ok(GblVsockConnectionManager::new(VsockConnectionManager::new(vsock)));
            }
            Err(Error::Unsupported)
        })
        .as_ref()
        .map_err(|e| *e)?;
    Ok(())
}
