/*
 * Copyright (C) 2025 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! Rust crate for parsing Android's super partition metadata, can be used
//! to resolve dynamic partitions in android.

#![no_std]

use zerocopy::{ByteSlice, FromBytes, Immutable, IntoBytes, Ref};

pub use liblp_bindgen::{
    LpMetadataExtent, LpMetadataGeometry, LpMetadataHeader, LpMetadataPartition,
    LpMetadataPartitionGroup, LpMetadataTableDescriptor, LP_METADATA_GEOMETRY_MAGIC,
    LP_METADATA_GEOMETRY_SIZE, LP_METADATA_HEADER_MAGIC, LP_PARTITION_RESERVED_BYTES,
};

/// Calculates the total size of metadata area in the super partition.
/// This function may overflow, in which case the program may panic,
/// depending on the compiler settings.
/// # Arguments
/// * `metadata_max_size` - The maximum size of a single metadata slot.
/// Usually 65536, obtained from super partition's geometry structure.
/// * `max_slots` - The maximum number of metadata slots. Usually 3.
/// Obtained from super partition's geometry structure.
/// # Returns
/// The total size of metadata area in bytes.
pub fn get_total_metadata_size(metadata_max_size: u32, max_slots: u32) -> u32 {
    LP_PARTITION_RESERVED_BYTES + (LP_METADATA_GEOMETRY_SIZE + metadata_max_size * max_slots) * 2
}

/// Represents the parsed metadata of a super partition.
/// Stored as references into the original buffer.
#[derive(PartialEq, Debug)]
pub struct LpMetadata<B: ByteSlice> {
    /// Geometry of super metadata
    pub geometry: Ref<B, LpMetadataGeometry>,
    /// Most important metadata structure, including partition
    pub metadata: Ref<B, LpMetadataHeader>,
    /// List of logical partitions in this super partition
    pub partitions: Ref<B, [LpMetadataPartition]>,
    /// List of all extents for all logical partitions. LpMetadataPartition
    /// contains indices into this array
    pub extents: Ref<B, [LpMetadataExtent]>,
    /// List of partition groups, referenced by LpMetadataPartition.group_index
    pub groups: Ref<B, [LpMetadataPartitionGroup]>,
}

/// Possible errors when parsing super partition metadata
#[derive(Debug, PartialEq)]
pub enum LiblpError {
    /// Input bytes buffer is too small
    BufferTooSmall,
    /// Invalid magic bytes in header
    InvalidMagic,
    /// Header contains invalid data
    InvalidHeader,
    /// Checksum verification failed
    InvalidChecksum,
}

fn get_primary_geometry_offset() -> usize {
    LP_PARTITION_RESERVED_BYTES as usize
}

fn get_backup_geometry_offset() -> usize {
    get_primary_geometry_offset() + LP_METADATA_GEOMETRY_SIZE as usize
}

fn get_primary_metadata_offset() -> usize {
    get_backup_geometry_offset() + LP_METADATA_GEOMETRY_SIZE as usize
}

fn get_backup_metadata_offset(metadata_max_size: u32, metadata_slot_count: u32) -> usize {
    get_primary_metadata_offset() + (metadata_max_size * metadata_slot_count) as usize
}

/// Trait for sha256 hashing in liblp. Users of liblp can choose their
/// own vendors. The init/update/final sequence will be called multiple
/// times during parsing.
pub trait HashOps {
    /// Initialize sha256 context, called before any update/final calls
    fn sha256_init(&mut self);
    /// Update sha256 context with data
    fn sha256_update(&mut self, data: &[u8]);
    /// Finalize sha256 and return the digest
    fn sha256_final(&mut self) -> [u8; 32];
}

fn sha256_bytes(data: &[u8], hasher: &mut impl HashOps) -> [u8; 32] {
    hasher.sha256_init();
    hasher.sha256_update(data);
    hasher.sha256_final()
}

/// Verifies the geometry structure's magic, size, and checksum
pub fn verify_geometry<T: ByteSlice>(
    geometry: Ref<T, LpMetadataGeometry>,
    hasher: &mut impl HashOps,
) -> Result<(), LiblpError> {
    if geometry.magic != LP_METADATA_GEOMETRY_MAGIC {
        return Err(LiblpError::InvalidMagic);
    }
    if geometry.struct_size as usize != core::mem::size_of::<LpMetadataGeometry>() {
        return Err(LiblpError::InvalidHeader);
    }
    let mut geometry_verify = Ref::read(&geometry);
    geometry_verify.checksum.fill(0);
    let bytes = geometry_verify.as_bytes();
    let digest = sha256_bytes(bytes, hasher);
    if geometry.checksum != digest {
        return Err(LiblpError::InvalidChecksum);
    }
    Ok(())
}

/// Verifies the metadata structure's magic, size, and checksum
pub fn verify_metadata<T: ByteSlice>(
    metadata: Ref<T, LpMetadataHeader>,
    hasher: &mut impl HashOps,
) -> Result<(), LiblpError> {
    if metadata.magic != LP_METADATA_HEADER_MAGIC {
        return Err(LiblpError::InvalidMagic);
    }
    if metadata.header_size as usize != core::mem::size_of::<LpMetadataHeader>() {
        return Err(LiblpError::InvalidHeader);
    }
    let mut metadata_verify = Ref::read(&metadata);
    metadata_verify.header_checksum.fill(0);
    let bytes: &[u8; core::mem::size_of::<LpMetadataHeader>()] =
        zerocopy::transmute_ref!(&metadata_verify);
    let digest = sha256_bytes(bytes, hasher);
    if metadata.header_checksum != digest {
        return Err(LiblpError::InvalidChecksum);
    }
    Ok(())
}

/// Parses the primary or backup geometry from the buffer
/// Input buffer should contain first N bytes of the super partition,
/// where N >= reserved bytes + 2 * geometry size
pub fn parse_geometry<'a>(
    buffer: &'a [u8],
    hasher: &mut impl HashOps,
) -> Result<Ref<&'a [u8], LpMetadataGeometry>, LiblpError> {
    let geometry_offset = get_primary_geometry_offset();
    let (geometry, _) = Ref::<_, LpMetadataGeometry>::from_prefix(
        buffer.get(geometry_offset..).ok_or(LiblpError::BufferTooSmall)?,
    )
    .map_err(|_| LiblpError::BufferTooSmall)?;

    if verify_geometry(geometry, hasher).is_ok() {
        return Ok(geometry);
    }
    let (backup_geometry, _) = Ref::<_, LpMetadataGeometry>::from_prefix(
        buffer.get(get_backup_geometry_offset()..).ok_or(LiblpError::BufferTooSmall)?,
    )
    .map_err(|_| LiblpError::BufferTooSmall)?;
    verify_geometry(backup_geometry, hasher)?;
    Ok(backup_geometry)
}

/// Result of parsing metadata, including the metadata structure
/// and the raw table buffer for further parsing
pub struct ParseMetadataResult<'a> {
    /// The metadata header structure, referencing into the original buffer
    pub metadata: Ref<&'a [u8], LpMetadataHeader>,
    /// Raw table buffer, which contains partition/extents/groups tables
    pub table_buffer: &'a [u8],
}

/// Verifies the table checksum against the metadata
fn verify_table_checksum<'a>(
    buffer: &'a [u8],
    metadata: Ref<&'a [u8], LpMetadataHeader>,
    metadata_offset: usize,
    hasher: &mut impl HashOps,
) -> Result<&'a [u8], LiblpError> {
    let table_start = metadata_offset + metadata.header_size as usize;
    let table_data = buffer
        .get(table_start..table_start + metadata.tables_size as usize)
        .ok_or(LiblpError::BufferTooSmall)?;
    let digest = sha256_bytes(table_data, hasher);
    if digest != metadata.tables_checksum {
        return Err(LiblpError::InvalidChecksum);
    }
    Ok(table_data)
}

/// Parses the primary or backup metadata from the buffer
/// Input buffer should contain first N bytes of the super partition,
/// where N >= reserved bytes + 2 * (geometry size + metadata size)
pub fn parse_metadata<'a>(
    buffer: &'a [u8],
    metadata_max_size: u32,
    metadata_slot_count: u32,
    hasher: &mut impl HashOps,
) -> Result<ParseMetadataResult<'a>, LiblpError> {
    let mut try_parse = |offset: usize| {
        let (metadata, _) = Ref::<_, LpMetadataHeader>::from_prefix(
            buffer.get(offset..).ok_or(LiblpError::BufferTooSmall)?,
        )
        .map_err(|_| LiblpError::BufferTooSmall)?;
        verify_metadata(metadata, hasher)?;
        let table_buffer = verify_table_checksum(buffer, metadata, offset, hasher)?;
        Ok(ParseMetadataResult { metadata, table_buffer })
    };
    try_parse(get_primary_geometry_offset())
        .or_else(|_| try_parse(get_backup_metadata_offset(metadata_max_size, metadata_slot_count)))
}

fn parse_table<'a, T>(
    all_tables: &'a [u8],
    desc: &LpMetadataTableDescriptor,
) -> Result<Ref<&'a [u8], [T]>, LiblpError>
where
    T: FromBytes,
    T: Immutable,
{
    let table_data = all_tables.get(desc.offset as usize..).ok_or(LiblpError::BufferTooSmall)?;
    let (table, _) = Ref::<_, [T]>::from_prefix_with_elems(table_data, desc.num_entries as usize)
        .map_err(|_| LiblpError::BufferTooSmall)?;
    Ok(table)
}

/// Maximum supported slots and metadata size for validation
/// We really only need 2 slots, but typical super partitions have 3 slots.
pub const MAX_SUPPORTED_SLOTS: u32 = 3;
/// The limits are chosen to match typical super partition layouts.
/// These limits can be increased, just uses more memory during parsing.
pub const MAX_SUPPORTED_METADATA_SIZE: u32 = 128 * 1024; // 128KB

/// Parse the super partition metadata from the buffer
pub fn parse<'a>(
    buffer: &'a [u8],
    hasher: &mut impl HashOps,
) -> Result<LpMetadata<&'a [u8]>, LiblpError> {
    let geometry = parse_geometry(buffer, hasher)?;
    if geometry.metadata_max_size == 0 || geometry.metadata_slot_count == 0 {
        return Err(LiblpError::InvalidHeader);
    }
    if geometry.metadata_slot_count > MAX_SUPPORTED_SLOTS
        || geometry.metadata_max_size > MAX_SUPPORTED_METADATA_SIZE
    {
        return Err(LiblpError::InvalidHeader);
    }

    let ParseMetadataResult { metadata, table_buffer } =
        parse_metadata(buffer, geometry.metadata_max_size, geometry.metadata_slot_count, hasher)?;

    let partitions_desc = &metadata.partitions;
    let partitions: Ref<_, [LpMetadataPartition]> = parse_table(table_buffer, partitions_desc)?;
    let extents_desc = &metadata.extents;
    let extents: Ref<_, [LpMetadataExtent]> = parse_table(table_buffer, extents_desc)?;

    let groups_desc = &metadata.groups;
    let groups: Ref<_, [LpMetadataPartitionGroup]> = parse_table(table_buffer, groups_desc)?;
    Ok(LpMetadata { geometry, metadata, partitions, extents, groups })
}
