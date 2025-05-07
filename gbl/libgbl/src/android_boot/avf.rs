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

use crate::{android_boot::load::split, constants::PAGE_SIZE, gbl_println, GblOps, KiB};
use avb::SlotVerifyData;
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

fn align_up(size: usize, alignment: usize) -> Result<usize> {
    Ok(SafeNum::from(size).round_up(alignment).try_into().map_err(Error::ArithmeticOverflow)?)
}

const fn align_up_const(size: usize, alignment: usize) -> usize {
    let offset = alignment.checked_sub(1).unwrap();
    size.checked_add(offset).unwrap() & !offset
}

/// Represents an object contains AVF verification data
pub(crate) trait AVFVerificationData {
    /// Returns the vendor hashtree digest if it exists.
    fn vendor_hashtree_digest(&self) -> Option<&[u8]>;
}

impl AVFVerificationData for SlotVerifyData<'_> {
    /// Extract the vendor hashtree digest from VB meta property.
    fn vendor_hashtree_digest(&self) -> Option<&[u8]> {
        // In order to successfully boot a Microdroid pVM with a vendor partition, the bootloader
        // must add the hashtree digest of the vendor image as a device tree property value.
        // See details:
        // https://cs.android.com/android/platform/superproject/main/+/main:packages/modules/Virtualization/docs/microdroid_vendor_modules.md,
        const HASHTREE_DIGEST_PROPNAME: &str = "com.android.build.microdroid-vendor.root_digest";
        self.vbmeta_data().iter().find_map(|data| data.get_property_value(HASHTREE_DIGEST_PROPNAME))
    }
}

/// Places pvmfw firmware binary and configuration data into reserved memory region
///
/// Copies the pvmfw binary, builds its configuration data, assembles them into a single blob, and
/// places it into a reserved memory region for later use by the hypervisor.
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
/// * `ops` - an implementation of `GblOps`
/// * `output_buffer` - the target load buffer.
/// * `pvmfw_binary` - a byte slice containing a preloaded pvmfw binary
/// * `verify_data` - an `AVFVerificationData` implementation (eg `SlotVerifyData`)
///
/// # Returns
///
/// * `Ok(usize)` - on success, the total size of the loaded image.
/// * `Err(InvalidArgument)` - if the pvmfw partition cannot be parsed
/// * `Err(BadBufferSize)` - if pvmfw binary cannot be extracted or the size data is invalid
/// * `Err(BufferTooSmall)` - of the pvmfw binary and config data doesn't fit into the target buffer
/// * `Err(ArithmeticOverflow)` - on overflow when calculating image buffer size
pub fn build_pvmfw_data_region<'a, 'b>(
    ops: &mut impl GblOps<'a, 'b>,
    output_buffer: &mut [u8],
    pvmfw_binary: &[u8],
    verify_data: &impl AVFVerificationData,
) -> Result<usize> {
    // Copy the binary to the start of pvmfw region
    let pvmfw_bin_size = pvmfw_binary.len();
    let (binary, config) = output_buffer
        .split_at_mut_checked(pvmfw_bin_size)
        .ok_or(Error::BufferTooSmall(Some(pvmfw_bin_size)))?;
    binary.copy_from_slice(pvmfw_binary);

    // Append the pvmfw configuration data and update host dt
    let config_size = write_pvmfw_config(ops, config, verify_data)?;
    // Size must be aligned to the page size used by the hypervisor
    let total_size = align_up(
        (SafeNum::from(pvmfw_bin_size) + config_size)
            .try_into()
            .map_err(Error::ArithmeticOverflow)?,
        PAGE_SIZE,
    )?;

    gbl_println!(ops, "AVF: init success");
    Ok(total_size)
}

/// Add a device tree node describing pvmfw memory carveout. This is default behavior, required for
/// pKVM, and can be overridden for other hypervisors by removing the
/// `/reserved-memory/pkvm_guest_firmware` node in `ops.fixup_device_tree`.
fn pkvm_describe_pvmfw_resvmem<'a, T>(fdt: &mut Fdt<T>, buffer: &[u8]) -> Result<()>
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

// TODO: Try to encapsulate building the pvmfw configuration buffer as suggested:
// http://aosp/3674715/comment/3fa5f59f_d119abc8/

/// Pvmfw configuration entry implementation; see:
/// https://cs.android.com/android/platform/superproject/main/+/main:packages/modules/Virtualization/guest/pvmfw/README.md
#[repr(C, packed)]
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, Immutable, KnownLayout, FromBytes, IntoBytes,
)]
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
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, Immutable, KnownLayout, FromBytes, IntoBytes,
)]
struct PvmfwConfHeader {
    magic: u32,
    version: u32,
    total_size: u32,
    flags: u32,
    entries: [PvmfwConfEntry; NUM_PVMFW_CONFIG_ENTRIES],
}

const_assert!(PvmfwConfHeader::ALIGNMENT >= align_of::<PvmfwConfHeader>());

type EntryBufsSizes = [(usize, usize); NUM_PVMFW_CONFIG_ENTRIES];

impl PvmfwConfHeader {
    const MAGIC: u32 = u32::from_ne_bytes(*b"pvmf");
    const DEFAULT_FLAGS: u32 = 0; // Flags field is currently unused and must be zero
    const ALIGNMENT: usize = KiB!(4);
    const PADDED_SIZE: usize = align_up_const(size_of::<Self>(), PvmfwConfEntry::ALIGNMENT);

    fn init_padded_prefix_mut<'a>(buffer: &'a mut [u8]) -> Result<(&'a mut Self, &'a mut [u8])> {
        if buffer.as_ptr().align_offset(PvmfwConfHeader::ALIGNMENT.into()) != 0 {
            return Err(Error::InvalidAlignment.into());
        }
        let (header_buf, remains) = split(buffer, Self::PADDED_SIZE)?;
        let (header, header_pad) =
            Self::mut_from_prefix(header_buf).map_err(|_| Error::BadBufferSize)?;
        header.magic = Self::MAGIC;
        header.version = Self::encode_pvmfw_config_version(1, 2);
        header.flags = Self::DEFAULT_FLAGS;
        header.total_size = Self::PADDED_SIZE.try_into()?;
        header_pad.fill(0u8);
        Ok((header, remains))
    }

    fn set_config_entries(&mut self, entry_sizes: EntryBufsSizes) -> Result<()> {
        let mut total_size = SafeNum::from(Self::PADDED_SIZE);

        for (entry, (entry_len, padded_len)) in self.entries.iter_mut().zip(entry_sizes) {
            if entry_len > padded_len || padded_len % PvmfwConfEntry::ALIGNMENT != 0 {
                return Err(Error::BadBufferSize.into());
            }
            entry.offset = total_size.try_into().map_err(Error::ArithmeticOverflow)?;
            entry.size = entry_len.try_into()?;
            total_size += padded_len;
        }
        self.total_size = total_size.try_into().map_err(Error::ArithmeticOverflow)?;
        Ok(())
    }

    const fn encode_pvmfw_config_version(major: u16, minor: u16) -> u32 {
        ((major as u32) << 16) | (minor as u32)
    }
}

/// Write the pvmfw configuration to the output buffer. Creates the configuration header and appends
/// the configuration entries.
fn write_pvmfw_config<'a, 'b>(
    ops: &mut impl GblOps<'a, 'b>,
    config_out: &mut [u8],
    verify_data: &impl AVFVerificationData,
) -> Result<usize> {
    let (header, entries) = PvmfwConfHeader::init_padded_prefix_mut(config_out)?;

    // Write pvmfw config entries
    let (ref_dt_len, ref_dt_padded_len, _rest) =
        pvmfw_build_reference_dt(ops, entries, verify_data)?;
    // Empty entries except reference dt
    let entry_sizes = [(0, 0), (0, 0), (0, 0), (ref_dt_len, ref_dt_padded_len)];

    // Finally, update header config entries
    header.set_config_entries(entry_sizes)?;
    Ok(header.total_size.try_into()?)
}

fn pad_entry_split_rest(buffer: &mut [u8], entry_size: usize) -> Result<(usize, &mut [u8])> {
    let padded_size = align_up(entry_size, PvmfwConfEntry::ALIGNMENT)?;
    let (padded_entry, rest) = split(buffer, padded_size)?;
    padded_entry[entry_size..].fill(0u8);
    Ok((padded_size, rest))
}

const REF_DT_AVF_PATH: &str = "/avf";
const HOST_DT_AVF_PATH: &str = "/avf/reference/avf";
const SK_PUB_KEY_PROP: &CStr = c"secretkeeper_public_key";
const VENDOR_HASH_PROP: &CStr = c"vendor_hashtree_descriptor_root_digest";

/// Write an FDT (the VM reference DT) to the output buffer and return its size, its padded size,
/// and the unused portion of the buffer.
fn pvmfw_build_reference_dt<'a, 'b, 'c>(
    ops: &mut impl GblOps<'a, 'b>,
    output_buffer: &'c mut [u8],
    verify_data: &impl AVFVerificationData,
) -> Result<(usize, usize, &'c mut [u8])> {
    let mut ref_dt = Fdt::new_empty(&mut output_buffer[..])?;
    write_ref_dt_properties(ops, &mut ref_dt, REF_DT_AVF_PATH, verify_data)?;
    ref_dt.shrink_to_fit()?;
    let entry_size = ref_dt.size()?;
    let (entry_padded_size, rest) = pad_entry_split_rest(output_buffer, entry_size)?;
    Ok((entry_size, entry_padded_size, rest))
}

fn write_ref_dt_properties<'a, 'b, 'c, T>(
    ops: &mut impl GblOps<'a, 'b>,
    target_dt: &mut Fdt<T>,
    avf_node_path: &str,
    verify_data: &impl AVFVerificationData,
) -> Result<()>
where
    T: AsMut<[u8]> + AsRef<[u8]>,
{
    // AVF node must be present
    target_dt.ensure_node(avf_node_path)?;

    // Write the secretkeeper public key.
    // The maximum size of a key returned from a Secretkeeper implementation depends on the
    // type of the key, so reserve enough for the largest key. See details:
    // https://cs.android.com/android/platform/superproject/main/+/main:external/trusty/bootloader/ql-tipc/include/trusty/secretkeeper.h;l=44
    // This property is required if updateable VMs feature is supported and Secretkeeper HAL
    // implementation exists.
    const SECRETKEEPER_KEY_BUFFER_SIZE: usize = 128;

    let mut sk_buf = [0u8; SECRETKEEPER_KEY_BUFFER_SIZE];
    let secretkeeper_public_key = ops.avf_read_secretkeeper_public_key(&mut sk_buf).ok().flatten();
    if let Some(sk_key) = secretkeeper_public_key {
        target_dt.set_property(avf_node_path, SK_PUB_KEY_PROP, sk_key)?;
    }

    // Write the vendor hashtree digest value. This property is required for vendor image
    // verification in Microdroid (eg for VM device assignment use). Otherwise it can be left out.
    let hashtree_digest = verify_data.vendor_hashtree_digest();
    if let Some(digest) = hashtree_digest {
        target_dt.set_property(avf_node_path, VENDOR_HASH_PROP, digest)?;
    }
    Ok(())
}

/// Add AVF-specific properties to host FDT
pub fn avf_fixup_host_dt<'a, 'b, 'c, T>(
    ops: &mut impl GblOps<'a, 'b>,
    host_dt: &mut Fdt<T>,
    pvmfw_buf: &[u8],
    verify_data: &impl AVFVerificationData,
) -> Result<()>
where
    T: AsMut<[u8]> + AsRef<[u8]>,
{
    pkvm_describe_pvmfw_resvmem(host_dt, pvmfw_buf)?;
    write_ref_dt_properties(ops, host_dt, HOST_DT_AVF_PATH, verify_data)
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
    use crate::{
        constants::PVMFW_DATA_ALIGNMENT,
        ops::test::{FakeGblOps, FakeGblOpsStorage},
    };
    use libtestutils::AlignedBuffer;

    struct TestVerifyData<T: AsRef<[u8]>>(Option<T>);

    impl<T: AsRef<[u8]>> AVFVerificationData for TestVerifyData<T> {
        fn vendor_hashtree_digest(&self) -> Option<&[u8]> {
            self.0.as_ref().map(|t| t.as_ref())
        }
    }

    fn dummy_pvmfw_binary(fill_value: u8, fill_count: usize) -> Vec<u8> {
        let mut pvmfw_bin_buf = vec![fill_value; fill_count];
        pvmfw_bin_buf.resize(align_up(fill_count, PAGE_SIZE).unwrap(), 0);
        pvmfw_bin_buf
    }

    /// Returns a test pvmfw partition
    pub(crate) fn dummy_pvmfw_partition(fill_value: u8, fill_count: usize) -> (Vec<u8>, usize) {
        const HEADER_SIZE: usize = 4096;
        let binary = dummy_pvmfw_binary(fill_value, fill_count);
        let mut partition = vec![
            0x41, 0x4e, 0x44, 0x52, // magic
            0x4f, 0x49, 0x44, 0x21, // magic
            0x00, 0x00, 0x00, 0x00, // kernel size (binary size)
            0x00, 0x00, 0x00, 0x00, // ramdisk size
            0x8b, 0x01, 0x00, 0x1e, // os version
            0x2c, 0x06, 0x00, 0x00, // header size
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, 0x00, 0x00, 0x00, // reserved
            0x03, 0x00, 0x00, 0x00, // version
        ];
        partition.resize(HEADER_SIZE + binary.len(), 0);
        partition[8..12].copy_from_slice(&(fill_count as u32).to_le_bytes());
        partition[HEADER_SIZE..].copy_from_slice(&binary);
        (partition, align_up(fill_count, PAGE_SIZE).unwrap() + PvmfwConfHeader::PADDED_SIZE)
    }

    #[test]
    fn test_build_pvmfw_data_region() {
        let mut out_pvmfw_buf = AlignedBuffer::new(0x100000, PVMFW_DATA_ALIGNMENT);
        let storage = FakeGblOpsStorage::default();
        let mut ops = FakeGblOps::new(&storage);
        let testdigest = TestVerifyData(Some([1, 2, 3, 4, 5]));

        const FILL_VALUE: u8 = 0xAB;
        const FILL_COUNT: usize = 0xc00;
        let used_bytes = build_pvmfw_data_region(
            &mut ops,
            &mut out_pvmfw_buf,
            &dummy_pvmfw_binary(FILL_VALUE, FILL_COUNT),
            &testdigest,
        )
        .unwrap();
        assert!(used_bytes > PAGE_SIZE);
        assert!(used_bytes % PAGE_SIZE == 0);
        assert!(&out_pvmfw_buf[..FILL_COUNT].iter().all(|&b| b == FILL_VALUE));
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
        assert_eq!(&reg_prop[..8], (buf.as_ptr() as usize).to_be_bytes());
        assert_eq!(&reg_prop[8..], buf.len().to_be_bytes());
    }

    #[test]
    fn test_write_pvmfw_config() {
        let mut buf = AlignedBuffer::new(1000, PVMFW_DATA_ALIGNMENT);
        let storage = FakeGblOpsStorage::default();
        let mut ops = FakeGblOps::new(&storage);
        ops.avf_is_supported = true;
        let testdigest = TestVerifyData(Some([1, 2, 3, 4, 5]));

        let sz = write_pvmfw_config(&mut ops, &mut buf, &testdigest).unwrap();
        let header = PvmfwConfHeader::ref_from_prefix(&buf).unwrap().0;
        let exp_refdt_size = 0xd3u32;
        let expected_entries = [
            PvmfwConfEntry { offset: PvmfwConfHeader::PADDED_SIZE as u32, size: 0 },
            PvmfwConfEntry { offset: PvmfwConfHeader::PADDED_SIZE as u32, size: 0 },
            PvmfwConfEntry { offset: PvmfwConfHeader::PADDED_SIZE as u32, size: 0 },
            PvmfwConfEntry { offset: PvmfwConfHeader::PADDED_SIZE as u32, size: exp_refdt_size },
        ];
        let expected_header = PvmfwConfHeader {
            magic: PvmfwConfHeader::MAGIC,
            version: PvmfwConfHeader::encode_pvmfw_config_version(1, 2),
            total_size: sz as u32,
            flags: PvmfwConfHeader::DEFAULT_FLAGS,
            entries: expected_entries,
        };
        assert_eq!(header, &expected_header);
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

    #[test]
    fn test_fixup_host_dt() {
        let test_digest = [5u8; 64];
        let digest = TestVerifyData(Some(&test_digest));
        let sk_key = FakeGblOps::GBL_TEST_AVF_SECRET_KEEPER_PUBLIC_KEY;
        let mut ops = FakeGblOps::new(&[][..]);
        ops.avf_is_supported = true;
        let dummy_buf = [0u8; 4];

        let mut hostdt_buf = AlignedBuffer::new(1024, 8);
        let mut fdt = Fdt::new_empty(&mut hostdt_buf[..]).unwrap();

        avf_fixup_host_dt(&mut ops, &mut fdt, &dummy_buf, &digest).unwrap();

        assert_eq!(fdt.get_property(HOST_DT_AVF_PATH, VENDOR_HASH_PROP).unwrap(), test_digest);
        assert_eq!(fdt.get_property(HOST_DT_AVF_PATH, SK_PUB_KEY_PROP).unwrap(), sk_key);
    }

    #[test]
    fn test_write_reference_dt() {
        let test_digest = [5u8; 64];
        let digest = TestVerifyData(Some(&test_digest));
        let sk_key = FakeGblOps::GBL_TEST_AVF_SECRET_KEEPER_PUBLIC_KEY;
        let mut ops = FakeGblOps::new(&[][..]);
        ops.avf_is_supported = true;

        let mut refdt_buf = AlignedBuffer::new(1024, 8);

        let (ref_dt_size, ref_dt_padded_size, _) =
            pvmfw_build_reference_dt(&mut ops, &mut refdt_buf, &digest).unwrap();
        assert!(ref_dt_padded_size % PvmfwConfEntry::ALIGNMENT == 0);

        let refdt = Fdt::new(&refdt_buf[..ref_dt_size]).unwrap();
        assert_eq!(refdt.get_property(REF_DT_AVF_PATH, VENDOR_HASH_PROP).unwrap(), test_digest);
        assert_eq!(refdt.get_property(REF_DT_AVF_PATH, SK_PUB_KEY_PROP).unwrap(), sk_key);
    }

    #[test]
    fn test_write_empty_reference_dt() {
        let digest = TestVerifyData::<[u8; 0]>(None);
        let mut ops = FakeGblOps::new(&[][..]);
        ops.avf_is_supported = false;

        let mut refdt_buf = AlignedBuffer::new(1024, 8);
        let (ref_dt_size, ref_dt_padded_size, _) =
            pvmfw_build_reference_dt(&mut ops, &mut refdt_buf, &digest).unwrap();
        assert!(ref_dt_padded_size % PvmfwConfEntry::ALIGNMENT == 0);

        let refdt = Fdt::new(&refdt_buf[..ref_dt_size]).unwrap();
        refdt.get_property(REF_DT_AVF_PATH, VENDOR_HASH_PROP).unwrap_err();
        refdt.get_property(REF_DT_AVF_PATH, SK_PUB_KEY_PROP).unwrap_err();
    }
}
