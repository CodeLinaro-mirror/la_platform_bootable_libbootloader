// Copyright 2025, The Android Open Source Project
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

//! Android virtualization framework support for GBL

use super::load::BootImageV3Info;
use crate::{constants::PAGE_SIZE, gbl_println, GblOps, KiB};
use bootparams::bootconfig::BootConfigBuilder;
use core::{
    ffi::CStr,
    fmt::Write,
    mem::{align_of, size_of},
};
use fdt::{fdt_encode_cell_sized_property, std_props, Fdt};
use liberror::{Error, Result};
use safemath::SafeNum;
use static_assertions::const_assert;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub const DEFAULT_PVMFW_PART_NAME_CSTR: &CStr = c"pvmfw";
const NUM_PVMFW_CONFIG_ENTRIES: usize = 4;

type EntryBufsArray<'a> = [&'a [u8]; NUM_PVMFW_CONFIG_ENTRIES];

fn align_up(size: usize, alignment: usize) -> Result<usize> {
    Ok(SafeNum::from(size).round_up(alignment).try_into().map_err(Error::ArithmeticOverflow)?)
}

const fn align_up_const(size: usize, alignment: usize) -> usize {
    let offset = alignment.checked_sub(1).unwrap();
    size.checked_add(offset).unwrap() & !offset
}

/// Places pvmfw firmware binary into reserved memory region
///
/// Assembles the pvmfw binary and its configuration data into a single blob, and places it into a
/// reserved memory region for later use by the hypervisor.
///
/// As per the requirements outlined at
/// https://cs.android.com/android/platform/superproject/main/+/main:packages/modules/Virtualization/guest/pvmfw/README.md,
/// pvmfw expects to have been loaded at 4KiB-aligned address. Therefore we respect this alignment
/// when loading the binary image. The bootloader is expected to describe the region using a
/// reserved memory device tree node, with both address and size properly aligned to the page size
/// used by the hypervisor. The configuration data must be 4KiB-aligned as well, while each config
/// entry must be 8b-aligned.
///
/// See more details of pvmfw loading, the configuration data layout, and meaning of
/// individual config entries at the link above.
///
/// # Arguments
///
/// * `pvmfw_partition_buf` - a byte slice containing a preloaded pvmfw partition.
/// * `pvmfw_load_buf` - The target load buffer.
/// * `entries` - an array of individual pvmfw configuration entries (as byte slices) - appended to
/// the configuration header unchanged.
///
/// # Returns
///
/// * `Ok(usize)` - on success, the total size of the loaded image.
/// data structure (the binary and appended configuration data)
/// * `Err(InvalidArgument)` - if the pvmfw partition cannot be parsed
/// * `Err(BadBufferSize)` - if pvmfw binary cannot be extracted or the size data is invalid
/// * `Err(BufferTooSmall)` - of the pvmfw binary and config data doesn't fit into the target buffer
/// * `Err(ArithmeticOverflow)` - on overflow when calculating image buffer size
pub fn pvmfw_place_in_memory(
    pvmfw_partition_buf: &[u8],
    pvmfw_load_buf: &mut [u8],
    entries: EntryBufsArray,
) -> Result<usize> {
    // Parse the partition header an extract the pvmfw binary
    let info = BootImageV3Info::new(pvmfw_partition_buf)?;
    let pvmfw_bin =
        pvmfw_partition_buf.get(info.kernel_range.clone()).ok_or(Error::BadBufferSize)?;
    let pvmfw_bin_size = pvmfw_bin.len();
    assert!(pvmfw_bin_size % PvmfwConfHeader::ALIGNMENT == 0, "expected 4k aligned buffer");

    let image_size = calc_pvmfw_data_image_size(pvmfw_bin_size, &entries)?;
    let pvmfw_load_buf =
        pvmfw_load_buf.get_mut(..image_size).ok_or(Error::BufferTooSmall(Some(image_size)))?;
    let (bin, config) = pvmfw_load_buf
        .split_at_mut_checked(pvmfw_bin.len())
        .ok_or(Error::BufferTooSmall(Some(pvmfw_bin.len())))?;
    bin.clone_from_slice(pvmfw_bin);
    // Append the rest of the configuration
    Ok(pvmfw_bin.len() + write_pvmfw_config(config, entries)?)
}

/// Add a device tree node describing pvmfw memory carveout. This is default behavior, required for
/// pKVM, and can be overridden for other hypervisors by removing the
/// `/reserved-memory/pkvm_guest_firmware` node in `ops.fixup_device_tree`.
pub fn pkvm_describe_pvmfw_resvmem<'a, T>(fdt: &mut Fdt<T>, buffer: &[u8]) -> Result<()>
where
    T: AsMut<[u8]> + AsRef<[u8]>,
{
    const RESVMEM_PATH: &str = "/reserved-memory";
    const PVMFW_RESVMEM_PATH: &str = "/reserved-memory/pkvm_guest_firmware";
    const MAX_REG_CELLS: usize = 8;
    const FDT_CELL_SIZE: usize = 4;

    let mut reg_buf = [0u8; MAX_REG_CELLS * FDT_CELL_SIZE];

    // Determine the number of u32 cells for 'reg' address and size, use default values if missing
    let addr_cells = fdt.get_property_u32(RESVMEM_PATH, std_props::ADDRESS_CELLS).unwrap_or(2u32);
    let size_cells = fdt.get_property_u32(RESVMEM_PATH, std_props::SIZE_CELLS).unwrap_or(1u32);

    // Serialize region address and size, and write DT node properties
    let reg_bytes = fdt_encode_cell_sized_property(
        &[(buffer.as_ptr() as usize), buffer.len()],
        &[addr_cells, size_cells],
        &mut reg_buf,
    )?;

    fdt.set_property(
        PVMFW_RESVMEM_PATH,
        std_props::COMPATIBLE,
        b"linux,pkvm-guest-firmware-memory\0",
    )?;
    fdt.set_property(PVMFW_RESVMEM_PATH, std_props::REG, &reg_buf[..reg_bytes])?;
    fdt.set_property(PVMFW_RESVMEM_PATH, std_props::NO_MAP, &[])?;
    Ok(())
}

/// Pvmfw configuration entry implementation; see:
/// https://cs.android.com/android/platform/superproject/main/+/main:packages/modules/Virtualization/guest/pvmfw/README.md
#[repr(C, packed)]
#[derive(Copy, Clone, Default, PartialEq, Eq, Immutable, KnownLayout, FromBytes, IntoBytes)]
struct PvmfwConfEntry {
    offset: u32,
    size: u32,
}

const_assert!(PvmfwConfEntry::ALIGNMENT >= align_of::<PvmfwConfEntry>());

impl PvmfwConfEntry {
    const ALIGNMENT: usize = 8;
}

/// Pvmfw configuration header implementation; see:
/// https://cs.android.com/android/platform/superproject/main/+/main:packages/modules/Virtualization/guest/pvmfw/README.md
#[repr(C, packed)]
#[derive(Copy, Clone, Default, PartialEq, Eq, Immutable, KnownLayout, FromBytes, IntoBytes)]
struct PvmfwConfHeader {
    magic: u32,
    version: u32,
    total_size: u32,
    flags: u32,
    entries: [PvmfwConfEntry; NUM_PVMFW_CONFIG_ENTRIES],
}

const_assert!(PvmfwConfHeader::ALIGNMENT >= align_of::<PvmfwConfHeader>());

impl PvmfwConfHeader {
    const MAGIC: u32 = u32::from_ne_bytes(*b"pvmf");
    const DEFAULT_FLAGS: u32 = 0; // Flags field is currently unused and must be zero
    const ALIGNMENT: usize = KiB!(4);
    const PADDED_SIZE: usize = align_up_const(size_of::<Self>(), PvmfwConfEntry::ALIGNMENT);

    fn make_config_entries(
        entry_bufs: EntryBufsArray,
    ) -> Result<([PvmfwConfEntry; NUM_PVMFW_CONFIG_ENTRIES], u32)> {
        let mut total_size = SafeNum::from(Self::PADDED_SIZE);
        let mut entries = [PvmfwConfEntry::default(); NUM_PVMFW_CONFIG_ENTRIES];
        for (i, e) in entry_bufs.iter().enumerate() {
            entries[i].offset = total_size.try_into().map_err(Error::ArithmeticOverflow)?;
            entries[i].size = e.len().try_into()?;
            total_size += align_up(e.len(), PvmfwConfEntry::ALIGNMENT)?;
        }
        Ok((entries, total_size.try_into().map_err(Error::ArithmeticOverflow)?))
    }

    const fn encode_pvmfw_config_version(major: u16, minor: u16) -> u32 {
        ((major as u32) << 16) | (minor as u32)
    }

    fn new(entries: EntryBufsArray) -> Result<Self> {
        let (entries, total_size) = Self::make_config_entries(entries)?;
        Ok(Self {
            magic: Self::MAGIC,
            version: Self::encode_pvmfw_config_version(1, 2),
            flags: Self::DEFAULT_FLAGS,
            total_size,
            entries,
        })
    }
}

/// Returns total size of pvmfw configuration data for given entries
fn calc_pvmfw_data_image_size(pvmfw_bin_size: usize, entries: &EntryBufsArray) -> Result<usize> {
    let mut total = SafeNum::from(pvmfw_bin_size) + PvmfwConfHeader::PADDED_SIZE;
    for e in entries {
        total += align_up(e.len(), PvmfwConfEntry::ALIGNMENT)?;
    }
    // Size must be aligned to the page size used by the hypervisor
    Ok(align_up(total.try_into().map_err(Error::ArithmeticOverflow)?, PAGE_SIZE)?)
}

/// Write the pvmfw configuration to the image buffer. Creates the configuration header and appends
/// the configuration entries.
fn write_pvmfw_config(config: &mut [u8], entries: EntryBufsArray) -> Result<usize> {
    let config_buf_len = config.len();
    let (header_buf, mut remains) = config
        .split_at_mut_checked(PvmfwConfHeader::PADDED_SIZE)
        .ok_or(Error::BufferTooSmall(Some(PvmfwConfHeader::PADDED_SIZE)))?;
    if header_buf.as_ptr().align_offset(PvmfwConfHeader::ALIGNMENT.into()) != 0 {
        return Err(Error::InvalidAlignment.into());
    }

    let header = PvmfwConfHeader::new(entries)?;
    header_buf.fill(0);
    header_buf[..header.as_bytes().len()].clone_from_slice(header.as_bytes());
    let mut entry_buf;
    // Append the entries after the header and add padding
    for entry in entries {
        let padded_size = align_up(entry.len(), PvmfwConfEntry::ALIGNMENT)?;
        (entry_buf, remains) = remains
            .split_at_mut_checked(padded_size)
            .ok_or(Error::BufferTooSmall(Some(padded_size)))?;
        entry_buf.fill(0);
        entry_buf[..entry.len()].clone_from_slice(entry.as_bytes());
    }
    Ok(config_buf_len - remains.len())
}

/// Add AVF-specific parameters to bootconfig
///
/// # Note
/// `androidboot.hypervisor.version` is free-form and should be set by vendor via bootconfig fixup
pub fn avf_update_bootconfig<'a, 'b>(
    ops: &mut impl GblOps<'a, 'b>,
    bootconfig: &mut BootConfigBuilder,
) -> core::result::Result<(), Error> {
    const PROTECTED_PROP: &str = "androidboot.hypervisor.protected_vm.supported";
    const UNPROTECTED_PROP: &str = "androidboot.hypervisor.vm.supported";

    for prop in [PROTECTED_PROP, UNPROTECTED_PROP] {
        if bootconfig.config_str().contains(prop) {
            gbl_println!(
                ops,
                "WARNING: Unexpected property `{prop}` is detected in the platform \
                bootconfig, this will not be supported by GBL in the future versions and will \
                cause GBL boot failure."
            );
        } else {
            write!(bootconfig, "{prop}=true\n")?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use crate::{constants::PVMFW_DATA_ALIGNMENT, ops::test::FakeGblOps};
    use libtestutils::AlignedBuffer;

    /// Returns a test pvmfw partition and its expected load size
    pub(crate) fn dummy_pvmfw_partition(fill_value: u8) -> (Vec<u8>, usize) {
        const HEADER_SIZE: usize = 4096;
        const PARTITION_SIZE: usize = 8192;

        let mut partition = vec![
            0x41, 0x4e, 0x44, 0x52, // magic
            0x4f, 0x49, 0x44, 0x21, // magic
            0x00, 0x0c, 0x00, 0x00, // kernel size (binary size)
            0x00, 0x00, 0x00, 0x00, // ramdisk size
            0x8b, 0x01, 0x00, 0x1e, // os version
            0x2c, 0x06, 0x00, 0x00, // header size
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, 0x00, 0x00, 0x00, // reserved
            0x03, 0x00, 0x00, 0x00, // version
        ];
        partition.resize(PARTITION_SIZE, 0);
        partition[HEADER_SIZE..HEADER_SIZE + 0xc00].fill(fill_value);
        (partition, PAGE_SIZE + PvmfwConfHeader::PADDED_SIZE)
    }

    #[test]
    fn test_pvmfw_place_in_memory() {
        let mut buffer = AlignedBuffer::new(0x100000, PVMFW_DATA_ALIGNMENT);
        const FILL_VALUE: u8 = 0xAB;
        let (pvmfw_partition, expected_sz) = dummy_pvmfw_partition(FILL_VALUE);
        let sz =
            pvmfw_place_in_memory(&pvmfw_partition, &mut buffer, [&[]; NUM_PVMFW_CONFIG_ENTRIES])
                .unwrap();
        assert_eq!(sz, expected_sz);
        assert!(&buffer[..0xc00].iter().all(|&b| b == FILL_VALUE));
    }

    #[test]
    fn test_pvmfw_place_in_memory_bad_header() {
        let mut buffer = AlignedBuffer::new(0x100000, PVMFW_DATA_ALIGNMENT);
        let pvmfw_partition = [0u8; 0x1000];
        assert!(pvmfw_place_in_memory(
            &pvmfw_partition,
            &mut buffer,
            [&[]; NUM_PVMFW_CONFIG_ENTRIES]
        )
        .is_err());
    }

    #[test]
    fn test_pkvm_describe_pvmfw_resvmem() {
        let buf = AlignedBuffer::new(10, PVMFW_DATA_ALIGNMENT);

        let init = include_bytes!("../../../libfdt/test/data/res_mem_min_dt.dtb").to_vec();
        let mut fdt_buf = vec![0u8; init.len() + 512];
        let mut fdt = Fdt::new_from_init(&mut fdt_buf[..], &init[..]).unwrap();

        assert_eq!(
            fdt.get_property("/reserved-memory", std_props::ADDRESS_CELLS).unwrap(),
            &[0x0, 0x0, 0x0, 0x2]
        );
        assert_eq!(
            fdt.get_property("/reserved-memory", std_props::SIZE_CELLS).unwrap(),
            &[0x0, 0x0, 0x0, 0x2]
        );
        assert!(fdt.get_property("/reserved-memory/pkvm_guest_firmware", std_props::REG).is_err());

        assert!(pkvm_describe_pvmfw_resvmem(&mut fdt, &buf).is_ok());
        assert_eq!(
            fdt.get_property("/reserved-memory/pkvm_guest_firmware", std_props::COMPATIBLE)
                .unwrap(),
            b"linux,pkvm-guest-firmware-memory\0",
        );
        assert_eq!(
            fdt.get_property("/reserved-memory/pkvm_guest_firmware", std_props::NO_MAP).unwrap(),
            &[]
        );
        let reg_prop =
            fdt.get_property("/reserved-memory/pkvm_guest_firmware", std_props::REG).unwrap();
        assert_eq!(&reg_prop[..8], (buf.as_ref().as_ptr() as usize).to_be_bytes());
        assert_eq!(&reg_prop[8..], buf.len().to_be_bytes());
    }

    #[test]
    fn test_write_pvmfw_config() {
        let mut imbuf = AlignedBuffer::new(1000, PVMFW_DATA_ALIGNMENT);

        const DATA_LEN: usize = 10;
        let data_len_padded = align_up(DATA_LEN, 8).unwrap();

        let data = [[1u8; DATA_LEN], [2u8; DATA_LEN], [3u8; DATA_LEN], [4u8; DATA_LEN]];
        let sz = write_pvmfw_config(&mut imbuf, [&data[0], &data[1], &data[2], &data[3]]).unwrap();

        let header = PvmfwConfHeader::ref_from_prefix(&imbuf).unwrap().0;
        let expected_entries = [
            PvmfwConfEntry { offset: PvmfwConfHeader::PADDED_SIZE as u32, size: DATA_LEN as u32 },
            PvmfwConfEntry {
                offset: (PvmfwConfHeader::PADDED_SIZE + data_len_padded) as u32,
                size: DATA_LEN as u32,
            },
            PvmfwConfEntry {
                offset: (PvmfwConfHeader::PADDED_SIZE + data_len_padded * 2) as u32,
                size: DATA_LEN as u32,
            },
            PvmfwConfEntry {
                offset: (PvmfwConfHeader::PADDED_SIZE + data_len_padded * 3) as u32,
                size: DATA_LEN as u32,
            },
        ];
        let expected_header = PvmfwConfHeader {
            magic: PvmfwConfHeader::MAGIC,
            version: PvmfwConfHeader::encode_pvmfw_config_version(1, 2),
            total_size: sz as u32,
            flags: PvmfwConfHeader::DEFAULT_FLAGS,
            entries: expected_entries,
        };

        assert!(header == &expected_header);
        for i in 0..NUM_PVMFW_CONFIG_ENTRIES {
            let offset = expected_entries[i].offset as usize;
            let size = expected_entries[i].size as usize;
            assert_eq!(&imbuf[offset..offset + size], data[i]);
            assert!(&imbuf[offset + size..offset + data_len_padded].iter().all(|&v| v == 0u8));
        }
    }

    #[test]
    fn test_write_empty_pvmfw_config() {
        let mut imbuf = AlignedBuffer::new(1000, PVMFW_DATA_ALIGNMENT);

        let sz = write_pvmfw_config(&mut imbuf, [&[]; NUM_PVMFW_CONFIG_ENTRIES]).unwrap();
        let header = PvmfwConfHeader::ref_from_prefix(&imbuf).unwrap().0;
        let expected_header = PvmfwConfHeader {
            magic: PvmfwConfHeader::MAGIC,
            version: PvmfwConfHeader::encode_pvmfw_config_version(1, 2),
            total_size: sz as u32,
            flags: PvmfwConfHeader::DEFAULT_FLAGS,
            entries: [
                PvmfwConfEntry { offset: PvmfwConfHeader::PADDED_SIZE as u32, size: 0 },
                PvmfwConfEntry { offset: PvmfwConfHeader::PADDED_SIZE as u32, size: 0 },
                PvmfwConfEntry { offset: PvmfwConfHeader::PADDED_SIZE as u32, size: 0 },
                PvmfwConfEntry { offset: PvmfwConfHeader::PADDED_SIZE as u32, size: 0 },
            ],
        };
        assert!(header == &expected_header);
        assert_eq!(sz, PvmfwConfHeader::PADDED_SIZE);
    }

    #[test]
    fn test_write_avf_bootconfig() {
        let protected = "androidboot.hypervisor.protected_vm.supported";
        let unprotected = "androidboot.hypervisor.vm.supported";
        let mut ops = FakeGblOps::new(&[][..]);
        let mut bootconf_buffer = [0u8; 128];
        let mut bootconfig = BootConfigBuilder::new(&mut bootconf_buffer).unwrap();

        let bootconf_str = bootconfig.config_str();
        assert!(!bootconf_str.contains(protected));
        assert!(!bootconf_str.contains(unprotected));

        avf_update_bootconfig(&mut ops, &mut bootconfig).unwrap();
        assert_eq!(bootconfig.config_str().matches(protected).count(), 1);
        assert_eq!(bootconfig.config_str().matches(unprotected).count(), 1);

        avf_update_bootconfig(&mut ops, &mut bootconfig).unwrap();
        assert_eq!(bootconfig.config_str().matches(protected).count(), 1);
        assert_eq!(bootconfig.config_str().matches(unprotected).count(), 1);
    }
}
