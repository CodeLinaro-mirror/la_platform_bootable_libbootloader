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

//! Definitions and parsing of the pvmfw configuration data format.
//!
//! The pvmfw data region consists of the pvmfw binary followed by configuration data: a header
//! and a sequence of blob entries. The format is defined by pvmfw; see:
//! https://cs.android.com/android/platform/superproject/main/+/main:packages/modules/Virtualization/guest/pvmfw/README.md

#![cfg_attr(not(test), no_std)]

use core::mem::{align_of, size_of};
use liberror::{Error, Result};
use safemath::SafeNum;
use static_assertions::{const_assert, const_assert_eq};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Number of configuration entries in a version 1.2 header.
pub const NUM_PVMFW_CONFIG_ENTRIES: usize = 4;

/// Pvmfw configuration entry; see:
/// https://cs.android.com/android/platform/superproject/main/+/main:packages/modules/Virtualization/guest/pvmfw/README.md
#[repr(C, packed)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Immutable, KnownLayout, FromBytes, IntoBytes)]
pub struct PvmfwConfEntry {
    /// Entry offset relative to the configuration data start.
    pub offset: u32,
    /// Entry size in bytes.
    pub size: u32,
}

const_assert!(PvmfwConfEntry::ALIGNMENT >= align_of::<PvmfwConfEntry>());
const_assert_eq!(size_of::<PvmfwConfEntry>(), 8);

impl PvmfwConfEntry {
    /// Required alignment of every entry within the configuration data.
    pub const ALIGNMENT: usize = 8;
}

/// Pvmfw configuration header; see:
/// https://cs.android.com/android/platform/superproject/main/+/main:packages/modules/Virtualization/guest/pvmfw/README.md
#[repr(C, packed)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Immutable, KnownLayout, FromBytes, IntoBytes)]
pub struct PvmfwConfHeader {
    /// Header magic; see [PvmfwConfHeader::MAGIC].
    pub magic: u32,
    /// Format version encoded as `(major << 16) | minor`.
    pub version: u32,
    /// Total size of the configuration data, including the header and entry padding.
    pub total_size: u32,
    /// Feature flags.
    pub flags: u32,
    /// Configuration entry table.
    pub entries: [PvmfwConfEntry; NUM_PVMFW_CONFIG_ENTRIES],
}

const_assert!(PvmfwConfHeader::DATA_ALIGNMENT >= align_of::<PvmfwConfHeader>());
const_assert_eq!(size_of::<PvmfwConfHeader>(), 48);

impl PvmfwConfHeader {
    /// Header magic value.
    pub const MAGIC: u32 = u32::from_ne_bytes(*b"pvmf");
    /// Flags field is currently unused.
    pub const DEFAULT_FLAGS: u32 = 0;
    /// Required alignment of the configuration data start.
    pub const DATA_ALIGNMENT: usize = 4096;
    /// Size of the header padded to the entry alignment; the offset of the first entry.
    pub const PADDED_SIZE: usize = size_of::<Self>().next_multiple_of(PvmfwConfEntry::ALIGNMENT);
    /// The configuration format version GBL produces and the only version parsing supports;
    /// the entry count and thus the header layout depend on the version.
    pub const SUPPORTED_VERSION: u32 = Self::encode_pvmfw_config_version(1, 2);

    const fn encode_pvmfw_config_version(major: u16, minor: u16) -> u32 {
        ((major as u32) << 16) | (minor as u32)
    }
}

/// Configuration entries defined by version 1.2 of the pvmfw configuration format.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PvmfwConfigEntryType {
    /// DICE chain handover.
    DiceHandover = 0,
    /// Debug policy DTBO.
    DebugPolicy = 1,
    /// VM DTBO.
    VmDtbo = 2,
    /// VM reference device tree.
    VmReferenceDt = 3,
}

/// Read-only view of pvmfw configuration data.
#[derive(Debug)]
pub struct PvmfwConfig<'a> {
    header: &'a PvmfwConfHeader,
    buffer: &'a [u8],
}

impl<'a> PvmfwConfig<'a> {
    /// Parses the configuration data at the start of `buffer`.
    ///
    /// `buffer` may extend past the configuration data; bytes beyond the total size declared
    /// by the header are ignored.
    ///
    /// # Returns
    ///
    /// * `Ok(PvmfwConfig)` - on success
    /// * `Err(BufferTooSmall)` - if `buffer` cannot hold the header or the declared total size
    /// * `Err(BadMagic)` - if the header magic is invalid
    /// * `Err(UnsupportedVersion)` - if the header version is not
    ///   [PvmfwConfHeader::SUPPORTED_VERSION]
    /// * `Err(InvalidInput)` - if the declared total size is smaller than the header
    pub fn from_bytes(buffer: &'a [u8]) -> Result<Self> {
        let header = PvmfwConfHeader::ref_from_prefix(buffer)
            .map_err(|_| Error::BufferTooSmall(Some(PvmfwConfHeader::PADDED_SIZE)))?
            .0;
        if header.magic != PvmfwConfHeader::MAGIC {
            return Err(Error::BadMagic);
        }
        if header.version != PvmfwConfHeader::SUPPORTED_VERSION {
            return Err(Error::UnsupportedVersion);
        }
        let total_size: usize = SafeNum::from(header.total_size).try_into()?;
        if total_size < PvmfwConfHeader::PADDED_SIZE {
            return Err(Error::InvalidInput);
        }
        let buffer = buffer.get(..total_size).ok_or(Error::BufferTooSmall(Some(total_size)))?;
        Ok(Self { header, buffer })
    }

    /// Returns the content of the given configuration entry; empty for absent entries.
    ///
    /// # Returns
    ///
    /// * `Ok(&[u8])` - the entry content
    /// * `Err(OutOfRange)` - if the entry range lies outside the configuration data
    pub fn entry(&self, entry: PvmfwConfigEntryType) -> Result<&'a [u8]> {
        let PvmfwConfEntry { offset, size } = self.header.entries[entry as usize];
        let start: usize = SafeNum::from(offset).try_into().map_err(|_| Error::OutOfRange)?;
        let end: usize = (SafeNum::from(start) + size).try_into().map_err(|_| Error::OutOfRange)?;
        self.buffer.get(start..end).ok_or(Error::OutOfRange)
    }

    /// Total size of the configuration data in bytes, including the header and entry padding.
    pub fn total_size(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const HEADER_SIZE: usize = size_of::<PvmfwConfHeader>();
    /// Bytes appended after the configuration data that parsing must ignore.
    const TEST_TRAILING: [u8; 56] = [0xEE; 56];
    /// Entry contents sized to not be multiples of the entry alignment.
    const TEST_DICE_HANDOVER: [u8; 113] = [0xAA; 113];
    const TEST_REFERENCE_DT: [u8; 25] = [0xBB; 25];

    fn test_config_bytes_with(contents: [&[u8]; NUM_PVMFW_CONFIG_ENTRIES]) -> Vec<u8> {
        let mut header = PvmfwConfHeader {
            magic: PvmfwConfHeader::MAGIC,
            version: PvmfwConfHeader::SUPPORTED_VERSION,
            total_size: 0,
            flags: 0,
            entries: [PvmfwConfEntry { offset: 0, size: 0 }; NUM_PVMFW_CONFIG_ENTRIES],
        };
        let mut data = Vec::new();
        for (entry, content) in header.entries.iter_mut().zip(contents) {
            entry.offset = (PvmfwConfHeader::PADDED_SIZE + data.len()) as u32;
            entry.size = content.len() as u32;
            data.extend_from_slice(content);
            data.resize(data.len().next_multiple_of(PvmfwConfEntry::ALIGNMENT), 0);
        }
        header.total_size = (PvmfwConfHeader::PADDED_SIZE + data.len()) as u32;
        let mut buffer = header.as_bytes().to_vec();
        buffer.extend_from_slice(&data);
        buffer.extend_from_slice(&TEST_TRAILING);
        buffer
    }

    /// Configuration data with a DICE handover and a VM reference DT entry.
    fn test_config_bytes() -> Vec<u8> {
        test_config_bytes_with([&TEST_DICE_HANDOVER, &[], &[], &TEST_REFERENCE_DT])
    }

    fn test_total_size(buffer: &[u8]) -> usize {
        buffer.len() - TEST_TRAILING.len()
    }

    fn patch_header(buffer: &mut [u8], patch: impl FnOnce(&mut PvmfwConfHeader)) {
        patch(PvmfwConfHeader::mut_from_prefix(buffer).unwrap().0);
    }

    #[rustfmt::skip]
    const GOLDEN_HEADER: [u8; HEADER_SIZE] = [
        0x70, 0x76, 0x6d, 0x66, // magic "pvmf"
        0x02, 0x00, 0x01, 0x00, // version 1.2
        0xc8, 0x00, 0x00, 0x00, // total_size 200
        0x00, 0x00, 0x00, 0x00, // flags
        0x30, 0x00, 0x00, 0x00, 0x71, 0x00, 0x00, 0x00, // DICE handover: offset 48, size 113
        0xa8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // debug policy: offset 168, size 0
        0xa8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // VM DTBO: offset 168, size 0
        0xa8, 0x00, 0x00, 0x00, 0x19, 0x00, 0x00, 0x00, // VM reference DT: offset 168, size 25
    ];

    #[cfg(target_endian = "little")]
    #[test]
    fn test_parses_golden_header_bytes() {
        let mut buffer = test_config_bytes();
        buffer[..HEADER_SIZE].copy_from_slice(&GOLDEN_HEADER);
        let config = PvmfwConfig::from_bytes(&buffer).unwrap();
        assert_eq!(config.total_size(), 200);
        assert_eq!(config.entry(PvmfwConfigEntryType::DiceHandover).unwrap(), &TEST_DICE_HANDOVER);
        assert_eq!(config.entry(PvmfwConfigEntryType::VmReferenceDt).unwrap(), &TEST_REFERENCE_DT);
    }

    #[test]
    fn test_reads_entries() {
        let buffer = test_config_bytes();
        let config = PvmfwConfig::from_bytes(&buffer).unwrap();
        assert_eq!(config.total_size(), test_total_size(&buffer));
        assert_eq!(config.entry(PvmfwConfigEntryType::DiceHandover).unwrap(), &TEST_DICE_HANDOVER);
        assert_eq!(config.entry(PvmfwConfigEntryType::DebugPolicy).unwrap(), &[]);
        assert_eq!(config.entry(PvmfwConfigEntryType::VmDtbo).unwrap(), &[]);
        assert_eq!(config.entry(PvmfwConfigEntryType::VmReferenceDt).unwrap(), &TEST_REFERENCE_DT);
    }

    #[test]
    fn test_empty_entry_at_config_end() {
        let mut buffer = test_config_bytes();
        let total_size = test_total_size(&buffer) as u32;
        patch_header(&mut buffer, |header| {
            header.entries[PvmfwConfigEntryType::VmReferenceDt as usize] =
                PvmfwConfEntry { offset: total_size, size: 0 };
        });
        let config = PvmfwConfig::from_bytes(&buffer).unwrap();
        assert_eq!(config.entry(PvmfwConfigEntryType::VmReferenceDt).unwrap(), &[]);
    }

    #[test]
    fn test_reads_header_only_config() {
        let buffer = test_config_bytes_with([&[], &[], &[], &[]]);
        let config = PvmfwConfig::from_bytes(&buffer).unwrap();
        assert_eq!(config.total_size(), PvmfwConfHeader::PADDED_SIZE);
        assert_eq!(config.entry(PvmfwConfigEntryType::DiceHandover).unwrap(), &[]);
        assert_eq!(config.entry(PvmfwConfigEntryType::VmReferenceDt).unwrap(), &[]);
    }

    #[test]
    fn test_rejects_buffer_too_small_for_header() {
        let buffer = test_config_bytes();
        assert_eq!(
            PvmfwConfig::from_bytes(&buffer[..HEADER_SIZE - 1]).unwrap_err(),
            Error::BufferTooSmall(Some(HEADER_SIZE))
        );
    }

    #[test]
    fn test_rejects_bad_magic() {
        let mut buffer = test_config_bytes();
        buffer[0] ^= 0xFF;
        assert_eq!(PvmfwConfig::from_bytes(&buffer).unwrap_err(), Error::BadMagic);
    }

    #[test]
    fn test_rejects_unsupported_version() {
        for version in [
            PvmfwConfHeader::encode_pvmfw_config_version(1, 1),
            PvmfwConfHeader::encode_pvmfw_config_version(2, 0),
        ] {
            let mut buffer = test_config_bytes();
            patch_header(&mut buffer, |header| header.version = version);
            assert_eq!(PvmfwConfig::from_bytes(&buffer).unwrap_err(), Error::UnsupportedVersion);
        }
    }

    #[test]
    fn test_rejects_total_size_smaller_than_header() {
        let mut buffer = test_config_bytes();
        patch_header(&mut buffer, |header| header.total_size = HEADER_SIZE as u32 - 1);
        assert_eq!(PvmfwConfig::from_bytes(&buffer).unwrap_err(), Error::InvalidInput);
    }

    #[test]
    fn test_rejects_truncated_config() {
        let buffer = test_config_bytes();
        let total_size = test_total_size(&buffer);
        assert_eq!(
            PvmfwConfig::from_bytes(&buffer[..total_size - 1]).unwrap_err(),
            Error::BufferTooSmall(Some(total_size))
        );
    }

    #[test]
    fn test_rejects_entry_exceeding_config() {
        let total_size = test_total_size(&test_config_bytes()) as u32;
        for (offset, size) in [
            (PvmfwConfHeader::PADDED_SIZE as u32, total_size),
            (total_size - 8, 16),
            (total_size + 1, 0),
            (u32::MAX, u32::MAX),
        ] {
            let mut buffer = test_config_bytes();
            patch_header(&mut buffer, |header| {
                header.entries[PvmfwConfigEntryType::DiceHandover as usize] =
                    PvmfwConfEntry { offset, size };
            });
            let config = PvmfwConfig::from_bytes(&buffer).unwrap();
            assert_eq!(
                config.entry(PvmfwConfigEntryType::DiceHandover).unwrap_err(),
                Error::OutOfRange
            );
        }
    }
}
