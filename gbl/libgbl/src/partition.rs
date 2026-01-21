// Copyright 2024, The Android Open Source Project
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

//! This file implements storage and partition logic for libgbl.

use crate::fastboot::sparse::{is_sparse_image, write_sparse_image, SparseRawWriter};
use arrayvec::ArrayVec;
use bytes::buf::UninitSlice;
use core::{
    cell::{RefCell, RefMut},
    ffi::CStr,
    fmt::{Arguments, Write},
    ops::{Deref, DerefMut},
};
use gbl_async::block_on;
use gbl_storage::{
    BlockInfo, BlockIo, BlockIoSync, Disk, Gpt, GptBuilder, GptSyncResult,
    Partition as GptPartition,
};
use liberror::Error;
use libutils::FormattedBytes;
use safemath::SafeNum;
use zerocopy::{FromBytes, IntoBytes};

/// Maximum name length for raw partition.
pub const RAW_PARTITION_NAME_LEN: usize = 72;

/// Wraps a bytes buffer containing a null-terminated C string
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, FromBytes, IntoBytes)]
pub struct RawName([u8; RAW_PARTITION_NAME_LEN]);

impl RawName {
    /// Creates a new instance with formatted string.
    pub fn new_formatted(args: Arguments) -> Result<Self, Error> {
        let mut buf = [0u8; RAW_PARTITION_NAME_LEN];
        let mut bytes = FormattedBytes::new(&mut buf[..RAW_PARTITION_NAME_LEN - 1]);
        Write::write_fmt(&mut bytes, args).unwrap();
        CStr::from_bytes_until_nul(&buf[..])?;
        Ok(Self(buf))
    }

    /// Creates a new instance from a Cstring
    fn new(name: &CStr) -> Result<Self, Error> {
        let mut buf = [0u8; RAW_PARTITION_NAME_LEN];
        name.to_str().map_err(|_| Error::InvalidInput)?;
        let name = name.to_bytes_with_nul();
        buf.get_mut(..name.len()).ok_or(Error::InvalidInput)?.clone_from_slice(name);
        Ok(Self(buf))
    }

    /// Decodes to a string.
    pub fn to_str(&self) -> &str {
        self.to_cstr().to_str().unwrap()
    }

    /// Gets as CStr.
    pub fn to_cstr(&self) -> &CStr {
        CStr::from_bytes_until_nul(&self.0[..]).unwrap()
    }
}

impl AsRef<str> for RawName {
    fn as_ref(&self) -> &str {
        self.to_str()
    }
}

impl TryFrom<&str> for RawName {
    type Error = Error;

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        Self::new_formatted(format_args!("{name}"))
    }
}

/// Represents a GBL partition.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Partition {
    /// Raw storage partition.
    Raw(RawName, u64),
    /// Gpt Partition.
    Gpt(GptPartition),
}

impl Partition {
    /// Returns the size.
    pub fn size(&self) -> Result<u64, Error> {
        let (start, end) = self.absolute_range()?;
        Ok((SafeNum::from(end) - start).try_into()?)
    }

    /// Returns the name.
    pub fn name(&self) -> Result<&str, Error> {
        Ok(match self {
            Partition::Gpt(gpt) => gpt.name().ok_or(Error::InvalidInput)?,
            Partition::Raw(name, _) => name.to_str(),
        })
    }

    /// Computes the absolute start and end offset for the partition in the whole block device.
    pub fn absolute_range(&self) -> Result<(u64, u64), Error> {
        Ok(match self {
            Partition::Gpt(gpt) => gpt.absolute_range()?,
            Partition::Raw(_, size) => (0, *size),
        })
    }

    /// Computes the absolute range of a sub window for the given relative offset and size.
    pub fn sub(&self, off: Option<u64>, sz: Option<u64>) -> Result<(u64, u64), Error> {
        let (start, end) = self.absolute_range()?;
        let off = off.unwrap_or(0);
        let abs_start = SafeNum::from(start) + off;
        let sz = sz.map_or(SafeNum::from(end) - abs_start, |v| v.into());
        let abs_end = u64::try_from(abs_start + sz)?;
        match abs_end <= end {
            true => Ok((abs_start.try_into()?, abs_end)),
            _ => Err(Error::OutOfRange),
        }
    }
}

/// Represents the partition table for a block device. It can either be a GPT partition table or a
/// single whole device raw partition.
enum PartitionTable<G> {
    Raw(RawName, u64),
    Gpt(G),
}

/// The status of block device
pub enum BlockStatus {
    /// Idle,
    Idle,
    /// An IO in progress.
    Pending,
}

impl BlockStatus {
    /// Converts to str.
    pub fn to_str(&self) -> &'static str {
        match self {
            BlockStatus::Idle => "idle",
            BlockStatus::Pending => "IO pending",
        }
    }
}

/// Represents a disk device that contains either GPT partitions or a single whole raw storage
/// partition.
pub struct GblDisk<D, G> {
    // Contains a `Disk` for block IO.
    //
    // `disk` and `partitions` are wrapped in RefCell because they may be shared by multiple async
    // blocks for operations such as parallel fastboot download/flashing. They are also wrapped
    // separately in order to make operations on each independent and parallel for use cases such
    // as getting partition info for `fastboot getvar` when disk IO is busy.
    disk: RefCell<D>,
    partitions: RefCell<PartitionTable<G>>,
    info_cache: BlockInfo,
}

impl<B, S, T> GblDisk<Disk<B, S>, Gpt<T>>
where
    B: BlockIo,
    S: DerefMut<Target = [u8]>,
    T: DerefMut<Target = [u8]>,
{
    /// Creates a new instance using the same disk and partition table where all IOs only go through
    /// `BlockIO::read_blocks_sync()', `BlockIO::write_blocks_sync()'. This effectively makes API
    /// blocking and makes sure backend provided optimized `BlockIO::read_blocks_sync()',
    /// `BlockIO::write_blocks_sync()' are used.
    pub fn as_sync(
        &self,
    ) -> Result<
        GblDisk<Disk<BlockIoSync<RefMut<'_, B>>, RefMut<'_, [u8]>>, Gpt<RefMut<'_, [u8]>>>,
        Error,
    > {
        let disk = Disk::transpose_ref_mut(self.get_disk()?).into_sync();
        let mut parts = self.partitions.try_borrow_mut().map_err(|_| Error::NotReady)?;
        Ok(match parts.deref_mut() {
            PartitionTable::Raw(v, _) => GblDisk::new_raw(disk, v.to_cstr()).unwrap(),
            PartitionTable::Gpt(_) => {
                let gpt = RefMut::map(parts, |v| match v {
                    PartitionTable::Gpt(v) => v,
                    _ => unreachable!(),
                });
                GblDisk::new_gpt(disk, Gpt::transpose_ref_mut(gpt))
            }
        })
    }

    /// Creates a new instance as a GPT device.
    pub fn new_gpt(mut disk: Disk<B, S>, gpt: Gpt<T>) -> Self {
        let info_cache = disk.io().info();
        Self { disk: disk.into(), info_cache, partitions: PartitionTable::Gpt(gpt).into() }
    }

    /// Creates a new instance as a raw storage partition.
    pub fn new_raw(mut disk: Disk<B, S>, name: &CStr) -> Result<Self, Error> {
        let info_cache = disk.io().info();
        Ok(Self {
            disk: disk.into(),
            info_cache,
            partitions: PartitionTable::Raw(RawName::new(name)?, info_cache.total_size()?).into(),
        })
    }

    /// Gets the cached `BlockInfo`.
    pub fn block_info(&self) -> BlockInfo {
        self.info_cache
    }

    /// Gets the block status.
    pub fn status(&self) -> BlockStatus {
        match self.disk.try_borrow_mut().ok() {
            None => BlockStatus::Pending,
            _ => BlockStatus::Idle,
        }
    }

    /// Borrows disk mutably.
    fn get_disk(&self) -> Result<RefMut<'_, Disk<B, S>>, Error> {
        self.disk.try_borrow_mut().map_err(|_| Error::NotReady)
    }

    /// Gets the block io object `B` from the disk.
    pub fn get_blk_io(&self) -> Result<RefMut<'_, B>, Error> {
        Ok(RefMut::map(self.get_disk()?, |v| v.io()))
    }

    /// Gets an instance of `PartitionIo` for a partition.
    ///
    /// If `part` is `None`, an IO for the whole block device is returned.
    pub fn partition_io(&self, part: Option<&str>) -> Result<PartitionIo<'_, B>, Error> {
        let (part_start, part_end) = self.find_partition(part)?.absolute_range()?;
        Ok(PartitionIo {
            disks: [Disk::transpose_ref_mut(self.get_disk()?)].into(),
            parts: [(0, part_start, part_end)].into(),
        })
    }

    /// Finds a partition.
    ///
    /// * If `part` is none, the method returns an unnamed `Partition` that represents the whole
    ///   raw storage.
    pub fn find_partition(&self, part: Option<&str>) -> Result<Partition, Error> {
        let Some(part) = part else {
            return Ok(Partition::Raw(RawName::new(c"").unwrap(), self.info_cache.total_size()?));
        };

        match self.partitions.try_borrow_mut().map_err(|_| Error::NotReady)?.deref() {
            PartitionTable::Gpt(gpt) => Ok(Partition::Gpt(gpt.find_partition(part)?)),
            PartitionTable::Raw(name, size) if name.to_str() == part => {
                Ok(Partition::Raw(*name, *size))
            }
            _ => Err(Error::NotFound),
        }
    }

    /// Get total number of partitions.
    pub fn num_partitions(&self) -> Result<usize, Error> {
        match self.partitions.try_borrow_mut().map_err(|_| Error::NotReady)?.deref() {
            PartitionTable::Raw(_, _) => Ok(1),
            PartitionTable::Gpt(gpt) => gpt.num_partitions(),
        }
    }

    /// Gets a partition by index.
    pub fn get_partition_by_idx(&self, idx: usize) -> Result<Partition, Error> {
        match self.partitions.try_borrow_mut().map_err(|_| Error::NotReady)?.deref() {
            PartitionTable::Raw(name, v) if idx == 0 => Ok(Partition::Raw(*name, *v)),
            PartitionTable::Gpt(gpt) => Ok(Partition::Gpt(gpt.get_partition(idx)?)),
            _ => Err(Error::InvalidInput),
        }
    }

    /// Syncs GPT if the partition type is GPT.
    ///
    /// # Returns
    ///
    /// * Returns `Ok(Some(sync_res))` if partition type is GPT and disk access is successful, where
    ///  `sync_res` contains the GPT verification and restoration result.
    /// * Returns `Ok(None)` if partition type is not GPT.
    /// * Returns `Err` in other cases.
    pub async fn sync_gpt(&self) -> Result<Option<GptSyncResult>, Error> {
        match self.partitions.try_borrow_mut().map_err(|_| Error::NotReady)?.deref_mut() {
            PartitionTable::Raw(_, _) => Ok(None),
            PartitionTable::Gpt(gpt) => {
                let mut blk = self.disk.try_borrow_mut().map_err(|_| Error::NotReady)?;
                // Don't repair GPT silently.
                // TODO(b/441574159): Provides a mechanism for platform to configure whether GPT
                // should be repaired.
                Ok(Some(blk.sync_gpt(gpt, false).await?))
            }
        }
    }

    /// Updates GPT to the block device and sync primary and secondary GPT.
    ///
    /// # Args
    ///
    /// * `mbr_primary`: A buffer containing the MBR block, primary GPT header and entries.
    /// * `resize`: If set to true, the method updates the last partition to cover the rest of the
    ///    storage.
    ///
    /// # Returns
    ///
    /// * Return `Err(Error::NotReady)` if device is busy.
    /// * Return `Err(Error::Unsupported)` if partition type is not GPT.
    /// * Return `Ok(())` new GPT is valid and device is updated and synced successfully.
    pub async fn update_gpt(&self, mbr_primary: &mut [u8], resize: bool) -> Result<(), Error> {
        match self.partitions.try_borrow_mut().map_err(|_| Error::NotReady)?.deref_mut() {
            PartitionTable::Raw(_, _) => Err(Error::Unsupported),
            PartitionTable::Gpt(gpt) => {
                let mut blk = self.disk.try_borrow_mut().map_err(|_| Error::NotReady)?;
                blk.update_gpt(mbr_primary, resize, gpt).await
            }
        }
    }

    /// Erases GPT on the disk.
    ///
    /// # Returns
    ///
    /// * Return `Err(Error::NotReady)` if device is busy.
    /// * Return `Err(Error::Unsupported)` if partition type is not GPT.
    pub async fn erase_gpt(&self) -> Result<(), Error> {
        match self.partitions.try_borrow_mut().map_err(|_| Error::NotReady)?.deref_mut() {
            PartitionTable::Raw(_, _) => Err(Error::Unsupported),
            PartitionTable::Gpt(gpt) => {
                let mut disk = self.disk.try_borrow_mut().map_err(|_| Error::NotReady)?;
                disk.erase_gpt(gpt).await
            }
        }
    }

    /// Creates an instance of GptBuilder.
    pub fn gpt_builder(
        &self,
    ) -> Result<GptBuilder<RefMut<'_, Disk<B, S>>, RefMut<'_, Gpt<T>>>, Error> {
        let mut parts = self.partitions.try_borrow_mut().map_err(|_| Error::NotReady)?;
        match parts.deref_mut() {
            PartitionTable::Raw(_, _) => Err(Error::Unsupported),
            PartitionTable::Gpt(_) => {
                let gpt = RefMut::map(parts, |v| match v {
                    PartitionTable::Gpt(v) => v,
                    _ => unreachable!(),
                });
                Ok(GptBuilder::new(self.get_disk()?, gpt)?.0)
            }
        }
    }
}

/// An alias to `MultiPartitionIo` with `N = 1`.
pub type PartitionIo<'a, B> = MultiPartitionIo<'a, B, 1>;

/// `MultiPartitionIo` provides read/write APIs to one or more partitions.
pub struct MultiPartitionIo<'a, B: BlockIo, const N: usize> {
    disks: ArrayVec<Disk<RefMut<'a, B>, RefMut<'a, [u8]>>, N>,
    // (Index in `disks`, partition start, partition end)
    parts: ArrayVec<(usize, u64, u64), N>,
}

impl<'a, B: BlockIo, const N: usize> MultiPartitionIo<'a, B, N> {
    /// Checks the read/write parameters and returns the absolute offsets for read/write in each
    /// partition.
    fn check_rw_range(
        &self,
        off: u64,
        size: impl Into<SafeNum>,
    ) -> Result<ArrayVec<u64, N>, Error> {
        let size = size.into();
        let mut res = ArrayVec::default();
        for (_, part_start, part_end) in &self.parts {
            let ab_range_end = SafeNum::from(*part_start) + off + size;
            // Checks overflow by computing the difference between range end and partition end and
            // making sure it succeeds.
            let abs_off = (SafeNum::from(*part_end) - ab_range_end)
                .try_into()
                .and_then(|_: u64| (SafeNum::from(*part_start) + off).try_into())
                .map_err(|_| Error::OutOfRange)?;
            res.push(abs_off);
        }
        Ok(res)
    }

    /// Reads from the partition.
    pub async fn read<'b>(
        &mut self,
        off: u64,
        out: impl Into<&'b mut UninitSlice>,
    ) -> Result<(), Error> {
        if self.parts.len() > 1 {
            return Err(Error::InvalidState);
        }
        let out = out.into();
        let abs_off = self.check_rw_range(off, out.len())?[0];
        self.disks[self.parts[0].0].read(abs_off, out).await
    }

    /// Writes to the partition.
    pub async fn write(&mut self, off: u64, data: &mut [u8]) -> Result<(), Error> {
        let abs_offs = self.check_rw_range(off, data.len())?;
        for ((disk_idx, _, _), abs_off) in self.parts.iter().zip(abs_offs.iter()) {
            self.disks[*disk_idx].write(*abs_off, data).await?;
        }
        Ok(())
    }

    /// Writes sparse image to the partition.
    pub async fn write_sparse(&mut self, off: u64, img: &mut [u8]) -> Result<(), Error> {
        let sz = is_sparse_image(img).map_err(|_| Error::InvalidInput)?.data_size();
        let abs_offs = self.check_rw_range(off, sz)?;
        write_sparse_image(img, &mut (&abs_offs, self)).await?;
        Ok(())
    }

    /// Writes zeroes to the partition.
    pub async fn zeroize(&mut self, scratch: &mut [u8]) -> Result<(), Error> {
        for (disk_idx, start, end) in self.parts.iter() {
            self.disks[*disk_idx]
                .fill(*start, end.checked_sub(*start).unwrap(), 0, scratch)
                .await?;
        }
        Ok(())
    }

    /// Performs io-specific erase.
    pub async fn erase(&mut self, scratch: &mut [u8]) -> Result<(), Error> {
        for (disk_idx, start, end) in self.parts.iter() {
            self.disks[*disk_idx].erase(*start, end.checked_sub(*start).unwrap(), scratch).await?;
        }
        Ok(())
    }

    /// Turns this IO into one for a subrange in the partition.
    pub fn sub(mut self, off: u64, sz: u64) -> Result<Self, Error> {
        self.check_rw_range(off, sz)?;
        for (_, part_start, part_end) in self.parts.iter_mut() {
            *part_start += off;
            *part_end = *part_start + sz;
        }
        Ok(self)
    }

    /// Returns the size in bytes of the smallest partition.
    pub fn size_bytes(&self) -> u64 {
        self.parts
            .iter()
            .map(|(_, start, end)| u64::try_from(SafeNum::from(*end) - *start).unwrap())
            .min()
            .unwrap_or(0)
    }
}

impl<'a, B: BlockIo> MultiPartitionIo<'a, B, 1> {
    /// Returns the size of the partition.
    pub fn size(&self) -> u64 {
        let (_, start, end) = self.parts[0];
        // Corrects by construction. Should not fail.
        end.checked_sub(start).unwrap()
    }

    /// Gets the block device.
    pub fn dev(&mut self) -> &mut Disk<RefMut<'a, B>, RefMut<'a, [u8]>> {
        &mut self.disks[0]
    }
}

// Implements `SparseRawWriter` for tuple (array of flash offsets, MultiPartitionIo)
impl<'a, B: BlockIo, const N: usize> SparseRawWriter
    for (&ArrayVec<u64, N>, &mut MultiPartitionIo<'a, B, N>)
{
    async fn write(&mut self, off: u64, data: &mut [u8]) -> Result<(), Error> {
        for ((disk_idx, _, _), abs_off) in self.1.parts.iter().zip(self.0.iter()) {
            self.1.disks[*disk_idx]
                .write((SafeNum::from(off) + *abs_off).try_into()?, data)
                .await?;
        }
        Ok(())
    }
}

/// Checks that a partition is unique.
///
/// Returns a pair `(<block device index>, `Partition`)` if the partition exists and is unique.
pub fn check_part_unique(
    devs: &'_ [GblDisk<
        Disk<impl BlockIo, impl DerefMut<Target = [u8]>>,
        Gpt<impl DerefMut<Target = [u8]>>,
    >],
    part: &str,
) -> Result<(usize, Partition), Error> {
    let mut filtered = devs
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.find_partition(Some(part)).ok().map(|v| (i, v)));
    match (filtered.next(), filtered.next()) {
        (Some(v), None) => Ok(v),
        (Some(_), Some(_)) => Err(Error::NotUnique),
        _ => Err(Error::NotFound),
    }
}

/// Creates a `MultiPartitionIo` given a list of (disk index, start, end) tuples.
pub fn create_multi_partition_io<'a, B: BlockIo, const N: usize>(
    devs: &'a [GblDisk<Disk<B, impl DerefMut<Target = [u8]>>, Gpt<impl DerefMut<Target = [u8]>>>],
    parts_info: ArrayVec<(usize, u64, u64), N>,
) -> Result<MultiPartitionIo<'a, B, N>, Error> {
    let mut parts_info = parts_info.clone();
    parts_info.sort();
    let mut parts = ArrayVec::new();
    let mut disks = ArrayVec::new();
    // Checks duplication.
    if !parts_info.windows(2).all(|v| v[0] != v[1]) {
        return Err(Error::InvalidInput);
    }
    for (i, (id, start, end)) in parts_info.iter().enumerate() {
        if i == 0 || parts_info[i - 1].0 != *id {
            disks.push(Disk::transpose_ref_mut(devs[*id].get_disk()?));
        }
        parts.push((disks.len().checked_sub(1).unwrap(), *start, *end));
    }
    Ok(MultiPartitionIo { disks, parts })
}

/// Searches and creates a `MultiPartitionIo` given a list of partitions.
pub fn find_multi_partition_io<'a, B: BlockIo, const N: usize>(
    devs: &'a [GblDisk<Disk<B, impl DerefMut<Target = [u8]>>, Gpt<impl DerefMut<Target = [u8]>>>],
    parts: &[impl AsRef<str>; N],
) -> Result<MultiPartitionIo<'a, B, N>, Error> {
    let mut parts_info = ArrayVec::new();
    for part in parts.iter().map(|v| v.as_ref()) {
        let (id, p) = check_part_unique(devs, part)?;
        let (start, end) = p.absolute_range()?;
        parts_info.push((id, start, end));
    }
    create_multi_partition_io(devs, parts_info)
}

/// Checks that a partition is unique among all block devices and reads from it.
pub async fn read_unique_partition<'a>(
    devs: &'_ [GblDisk<
        Disk<impl BlockIo, impl DerefMut<Target = [u8]>>,
        Gpt<impl DerefMut<Target = [u8]>>,
    >],
    part: &str,
    off: u64,
    out: impl Into<&'a mut UninitSlice>,
) -> Result<(), Error> {
    devs[check_part_unique(devs, part)?.0].partition_io(Some(part))?.read(off, out).await
}

/// Same as `read_unique_partition` but IO is blocking.
pub fn read_unique_partition_sync<'a>(
    devs: &'_ [GblDisk<
        Disk<impl BlockIo, impl DerefMut<Target = [u8]>>,
        Gpt<impl DerefMut<Target = [u8]>>,
    >],
    part: &str,
    off: u64,
    out: impl Into<&'a mut UninitSlice>,
) -> Result<(), Error> {
    block_on(
        devs[check_part_unique(devs, part)?.0].as_sync()?.partition_io(Some(part))?.read(off, out),
    )
}

/// Checks that a partition is unique among all block devices and writes to it.
pub async fn write_unique_partition(
    devs: &'_ [GblDisk<
        Disk<impl BlockIo, impl DerefMut<Target = [u8]>>,
        Gpt<impl DerefMut<Target = [u8]>>,
    >],
    part: &str,
    off: u64,
    data: &mut [u8],
) -> Result<(), Error> {
    devs[check_part_unique(devs, part)?.0].partition_io(Some(part))?.write(off, data).await
}

/// Syncs all GPT type partition devices.
pub async fn sync_gpt(
    devs: &'_ [GblDisk<
        Disk<impl BlockIo, impl DerefMut<Target = [u8]>>,
        Gpt<impl DerefMut<Target = [u8]>>,
    >],
) -> Result<(), Error> {
    for ele in &devs[..] {
        ele.sync_gpt().await?;
    }
    Ok(())
}

/// Splits a partition name into a pair of `(basename: &str, suffix: char)`.
///
/// Returns `None` if partition did not have slot suffix.
pub fn split_partition_suffix(part: &str) -> Option<(&str, char)> {
    let (name, suffix) = part.rsplit_once('_')?;
    if name.len() > 0 && suffix.len() == 1 && suffix.chars().next()?.is_ascii_lowercase() {
        Some((name, suffix.chars().next()?))
    } else {
        None
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use crate::ops::test::{FakeGblOpsStorage, TestGblDisk};
    use core::fmt::Debug;
    use gbl_async::block_on;

    /// Absolute start/end offset and size of "boot_a/b" partitions in
    /// "../../libstorage/test/gpt_test_1.bin"
    const BOOT_A_OFF: u64 = 17 * 1024;
    const BOOT_A_END: u64 = 25 * 1024;
    const BOOT_A_SZ: u64 = BOOT_A_END - BOOT_A_OFF;
    const BOOT_B_OFF: u64 = 25 * 1024;
    const BOOT_B_END: u64 = 37 * 1024;
    const BOOT_B_SZ: u64 = BOOT_B_END - BOOT_B_OFF;
    /// Total size of disk "../../libstorage/test/gpt_test_1.bin"
    const GPT_DISK_1_SZ: u64 = 64 * 1024;

    /// A helper to convert an integer into usize and panics on error.
    fn to_usize(val: impl TryInto<usize, Error = impl Debug>) -> usize {
        val.try_into().unwrap()
    }

    /// A helper to create a GPT type TestGblDisk
    fn gpt_disk(data: impl AsRef<[u8]>) -> TestGblDisk {
        let mut res = FakeGblOpsStorage::default();
        res.add_gpt_device(data);
        res.0.pop().unwrap()
    }

    /// A helper to create a raw disk partition type TestGblDisk
    fn raw_disk(name: &CStr, data: impl AsRef<[u8]>) -> TestGblDisk {
        let mut res = FakeGblOpsStorage::default();
        res.add_raw_device(name, data);
        res.0.pop().unwrap()
    }

    #[test]
    fn test_find_partition_gpt() {
        let gpt = gpt_disk(include_bytes!("../../libstorage/test/gpt_test_1.bin"));
        assert_eq!(block_on(gpt.sync_gpt()).unwrap(), Some(GptSyncResult::BothValid));

        let boot_a = gpt.find_partition(Some("boot_a")).unwrap();
        assert_eq!(boot_a.name().unwrap(), "boot_a");
        assert_eq!(boot_a.size().unwrap(), BOOT_A_SZ);
        assert_eq!(boot_a.absolute_range().unwrap(), (BOOT_A_OFF, BOOT_A_END));

        let boot_b = gpt.find_partition(Some("boot_b")).unwrap();
        assert_eq!(boot_b.name().unwrap(), "boot_b");
        assert_eq!(boot_b.size().unwrap(), BOOT_B_SZ);
        assert_eq!(boot_b.absolute_range().unwrap(), (BOOT_B_OFF, BOOT_B_END));

        let unnamed_whole = gpt.find_partition(None).unwrap();
        assert_eq!(unnamed_whole.name().unwrap(), "");
        assert_eq!(unnamed_whole.size().unwrap(), GPT_DISK_1_SZ);
        assert_eq!(unnamed_whole.absolute_range().unwrap(), (0, GPT_DISK_1_SZ));

        assert!(gpt.find_partition(Some("not-exist")).is_err());
    }

    #[test]
    fn test_find_partition_raw() {
        let disk = include_bytes!("../../libstorage/test/gpt_test_1.bin");
        let raw = raw_disk(c"raw", &disk);

        let raw_part = raw.find_partition(Some("raw")).unwrap();
        assert_eq!(raw_part.name().unwrap(), "raw");
        assert_eq!(raw_part.size().unwrap(), GPT_DISK_1_SZ);
        assert_eq!(raw_part.absolute_range().unwrap(), (0, GPT_DISK_1_SZ));

        let unnamed_whole = raw.find_partition(None).unwrap();
        assert_eq!(unnamed_whole.name().unwrap(), "");
        assert_eq!(unnamed_whole.size().unwrap(), GPT_DISK_1_SZ);
        assert_eq!(unnamed_whole.absolute_range().unwrap(), (0, GPT_DISK_1_SZ));

        assert!(raw.find_partition(Some("boot_a")).is_err());
    }

    /// A helper for testing partition read.
    ///
    /// Tests that the content read at `off..off+sz` is the same as `part_content[off..off+sz]`.
    fn test_part_read(
        blk: &TestGblDisk,
        part: Option<&str>,
        part_content: &[u8],
        off: u64,
        sz: u64,
    ) {
        let mut out = vec![0u8; to_usize(sz)];
        block_on(blk.partition_io(part).unwrap().read(off, &mut out[..])).unwrap();
        assert_eq!(out, part_content[to_usize(off)..][..out.len()].to_vec());

        // Reads using the `sub()` and then read approach.
        let mut out = vec![0u8; to_usize(sz)];
        let mut io = blk.partition_io(part).unwrap().sub(off, sz).unwrap();
        block_on(io.read(0, &mut out[..])).unwrap();
        assert_eq!(out, part_content[to_usize(off)..][..out.len()].to_vec());
    }

    #[test]
    fn test_read_partition_gpt() {
        let disk = include_bytes!("../../libstorage/test/gpt_test_1.bin");
        let gpt = gpt_disk(&disk[..]);
        assert_eq!(block_on(gpt.sync_gpt()).unwrap(), Some(GptSyncResult::BothValid));

        let expect_boot_a = include_bytes!("../../libstorage/test/boot_a.bin");
        test_part_read(&gpt, Some("boot_a"), expect_boot_a, 1, 1024);
        let expect_boot_b = include_bytes!("../../libstorage/test/boot_b.bin");
        test_part_read(&gpt, Some("boot_b"), expect_boot_b, 1, 1024);
        // Whole block read.
        test_part_read(&gpt, None, disk, 1, 1024);
    }

    #[test]
    fn test_read_partition_raw() {
        let disk = include_bytes!("../../libstorage/test/gpt_test_1.bin");
        let raw = raw_disk(c"raw", &disk);
        test_part_read(&raw, Some("raw"), disk, 1, 1024);
        test_part_read(&raw, None, disk, 1, 1024);
    }

    /// A helper for testing partition write.
    fn test_part_write(blk: &TestGblDisk, part: Option<&str>, off: u64, sz: u64) {
        // Reads the current partition content
        let mut part_content = vec![0u8; to_usize(blk.partition_io(part).unwrap().size())];
        block_on(blk.partition_io(part).unwrap().read(0, &mut part_content[..])).unwrap();

        // Flips all the bits in the target range and writes back.
        let seg = &mut part_content[to_usize(off)..][..to_usize(sz)];
        seg.iter_mut().for_each(|v| *v = !(*v));
        block_on(blk.partition_io(part).unwrap().write(off, seg)).unwrap();
        // Checks that data is written.
        test_part_read(blk, part, &part_content, off, sz);

        // Writes using the `sub()` and then write approach.
        let seg = &mut part_content[to_usize(off)..][..to_usize(sz)];
        seg.iter_mut().for_each(|v| *v = !(*v));
        block_on(blk.partition_io(part).unwrap().sub(off, sz).unwrap().write(0, seg)).unwrap();
        test_part_read(blk, part, &part_content, off, sz);
    }

    #[test]
    fn test_write_partition_gpt() {
        let gpt = gpt_disk(include_bytes!("../../libstorage/test/gpt_test_1.bin"));
        assert_eq!(block_on(gpt.sync_gpt()).unwrap(), Some(GptSyncResult::BothValid));
        test_part_write(&gpt, Some("boot_a"), 1, 1024);
        test_part_write(&gpt, Some("boot_b"), 1, 1024);
        test_part_write(&gpt, None, 1, 1024);
    }

    #[test]
    fn test_write_partition_raw() {
        let mut raw = raw_disk(c"raw", include_bytes!("../../libstorage/test/gpt_test_1.bin"));
        test_part_write(&mut raw, Some("raw"), 1, 1024);
        test_part_write(&mut raw, None, 1, 1024);
    }

    #[test]
    fn test_multi_part_write() {
        let mut devs = FakeGblOpsStorage::default();
        devs.add_gpt_device(include_bytes!("../../libstorage/test/gpt_test_1.bin"));
        devs.add_gpt_device(include_bytes!("../../libstorage/test/gpt_test_2.bin"));
        devs.add_raw_device(c"raw_0", [0xaau8; 4 * 1024]);
        devs.add_raw_device(c"raw_1", [0x55u8; 4 * 1024]);

        let parts = &["boot_a", "vendor_boot_a", "raw_0", "raw_1"];
        let mut data = [0x11u8; 1024];
        {
            let mut io = find_multi_partition_io(&devs, parts).unwrap();
            block_on(io.write(1024, &mut data)).unwrap();
        }

        let mut expect = vec![0u8; 1024 + 1024];
        expect[1024..][..data.len()].clone_from_slice(&data);
        let sz: u64 = data.len().try_into().unwrap();
        check_read_partition(&devs, "boot_a", &expect, 1024, sz);
        check_read_partition(&devs, "vendor_boot_a", &expect, 1024, sz);
        check_read_partition(&devs, "raw_0", &expect, 1024, sz);
        check_read_partition(&devs, "raw_1", &expect, 1024, sz);
    }

    #[test]
    fn test_read_write_partition_overflow() {
        let disk = include_bytes!("../../libstorage/test/gpt_test_1.bin");
        let gpt = gpt_disk(&disk[..]);
        assert_eq!(block_on(gpt.sync_gpt()).unwrap(), Some(GptSyncResult::BothValid));

        let mut part_io = gpt.partition_io(Some("boot_a")).unwrap();
        assert!(block_on(part_io.read(BOOT_A_END, &mut vec![0u8; 1][..])).is_err());
        assert!(block_on(part_io.read(BOOT_A_OFF, &mut vec![0u8; to_usize(BOOT_A_SZ) + 1][..]))
            .is_err());
        assert!(block_on(part_io.write(BOOT_A_END, &mut vec![0u8; 1][..])).is_err());
        assert!(block_on(part_io.write(BOOT_A_OFF, &mut vec![0u8; to_usize(BOOT_A_SZ) + 1][..]))
            .is_err());

        let raw = raw_disk(c"raw", &disk);
        let mut part_io = raw.partition_io(Some("raw")).unwrap();
        assert!(block_on(part_io.read(GPT_DISK_1_SZ, &mut vec![0u8; 1][..])).is_err());
        assert!(block_on(part_io.read(0, &mut vec![0u8; to_usize(GPT_DISK_1_SZ) + 1][..])).is_err());
        assert!(block_on(part_io.write(GPT_DISK_1_SZ, &mut vec![0u8; 1][..])).is_err());
        assert!(
            block_on(part_io.write(0, &mut vec![0u8; to_usize(GPT_DISK_1_SZ) + 1][..])).is_err()
        );
    }

    #[test]
    fn test_sub_overflow() {
        let disk = include_bytes!("../../libstorage/test/gpt_test_1.bin");
        let gpt = gpt_disk(&disk[..]);
        assert_eq!(block_on(gpt.sync_gpt()).unwrap(), Some(GptSyncResult::BothValid));
        assert!(gpt.partition_io(Some("boot_a")).unwrap().sub(0, BOOT_A_SZ + 1).is_err());
        assert!(gpt.partition_io(Some("boot_a")).unwrap().sub(1, BOOT_A_SZ).is_err());

        let raw = raw_disk(c"raw", &disk);
        assert!(raw.partition_io(Some("raw")).unwrap().sub(0, GPT_DISK_1_SZ + 1).is_err());
        assert!(raw.partition_io(Some("raw")).unwrap().sub(1, GPT_DISK_1_SZ).is_err());
    }

    #[test]
    fn test_sub_overflow_multi_part() {
        let mut devs = FakeGblOpsStorage::default();
        devs.add_gpt_device(include_bytes!("../../libstorage/test/gpt_test_1.bin"));
        devs.add_raw_device(c"raw_0", [0xaau8; 4 * 1024]);
        devs.add_raw_device(c"raw_1", [0x55u8; 3 * 1024]);

        let parts = &["boot_a", "boot_b"];
        assert!(BOOT_A_SZ <= BOOT_B_SZ);
        assert!(find_multi_partition_io(&devs, parts).unwrap().sub(0, BOOT_A_SZ + 1).is_err());
        assert!(find_multi_partition_io(&devs, parts).unwrap().sub(1, BOOT_A_SZ).is_err());

        let parts = &["raw_0", "raw_1"];
        assert!(find_multi_partition_io(&devs, parts).unwrap().sub(1, 3 * 1024).is_err());
        assert!(find_multi_partition_io(&devs, parts).unwrap().sub(0, 3 * 1024 + 1).is_err());
    }

    #[test]
    fn test_write_sparse() {
        let sparse_raw = include_bytes!("../testdata/sparse_test_raw.bin");
        let mut sparse = include_bytes!("../testdata/sparse_test.bin").to_vec();
        let raw = &vec![0u8; sparse_raw.len() + 512][..];
        let blk = raw_disk(c"raw", raw);
        block_on(
            blk.partition_io(Some("raw"))
                .unwrap()
                .sub(1, u64::try_from(raw.len() - 1).unwrap())
                .unwrap()
                .write_sparse(1, &mut sparse),
        )
        .unwrap();
        let mut expected = vec![0u8; raw.len()];
        expected[1 + 1..][..sparse_raw.len()].clone_from_slice(sparse_raw);
        test_part_read(&blk, Some("raw"), &expected, 1, sparse_raw.len().try_into().unwrap());
    }

    #[test]
    fn test_write_sparse_multi_part() {
        let sparse_raw = include_bytes!("../testdata/sparse_test_raw.bin");
        let mut sparse = include_bytes!("../testdata/sparse_test.bin").to_vec();
        let mut devs = FakeGblOpsStorage::default();
        devs.add_raw_device(c"raw_0", vec![0u8; sparse_raw.len() + 512]);
        devs.add_raw_device(c"raw_1", vec![0u8; sparse_raw.len() + 1024]);
        {
            let mut io = find_multi_partition_io(&devs, &["raw_0", "raw_1"]).unwrap();
            io = io.sub(1, u64::try_from(sparse_raw.len()).unwrap() + 1).unwrap();
            block_on(io.write_sparse(1, &mut sparse)).unwrap();
        }
        let mut expected = vec![0u8; sparse_raw.len() + 2];
        expected[1 + 1..][..sparse_raw.len()].clone_from_slice(sparse_raw);
        test_part_read(&devs[0], Some("raw_0"), &expected, 1, sparse_raw.len().try_into().unwrap());
        test_part_read(&devs[1], Some("raw_1"), &expected, 1, sparse_raw.len().try_into().unwrap());
    }

    #[test]
    fn test_write_sparse_not_sparse_image() {
        let sparse_raw = include_bytes!("../testdata/sparse_test_raw.bin");
        let mut sparse = include_bytes!("../testdata/sparse_test.bin").to_vec();
        sparse[0] = !sparse[0]; // Corrupt image.
        let raw = raw_disk(c"raw", vec![0u8; sparse_raw.len() + 512]);
        assert!(
            block_on(raw.partition_io(Some("raw")).unwrap().write_sparse(1, &mut sparse)).is_err()
        );
    }

    #[test]
    fn test_write_sparse_overflow_size() {
        let sparse_raw = include_bytes!("../testdata/sparse_test_raw.bin");
        let mut sparse = include_bytes!("../testdata/sparse_test.bin").to_vec();
        let raw = raw_disk(c"raw", vec![0u8; sparse_raw.len()]);
        assert!(
            block_on(raw.partition_io(Some("raw")).unwrap().write_sparse(1, &mut sparse)).is_err()
        );
    }

    #[test]
    fn test_partition_iter() {
        let raw = raw_disk(c"raw", vec![0u8; 1024]);
        assert_eq!(raw.num_partitions().unwrap(), 1);
        assert_eq!(raw.get_partition_by_idx(0).unwrap().name(), Ok("raw"));
        assert_eq!(raw.get_partition_by_idx(0).unwrap().size(), Ok(1024));

        let gpt = gpt_disk(include_bytes!("../../libstorage/test/gpt_test_1.bin"));
        block_on(gpt.sync_gpt()).unwrap();
        assert_eq!(gpt.num_partitions().unwrap(), 2);
        assert_eq!(gpt.get_partition_by_idx(0).unwrap().name().unwrap(), "boot_a");
        assert_eq!(gpt.get_partition_by_idx(0).unwrap().size().unwrap(), 0x2000);
        assert_eq!(gpt.get_partition_by_idx(1).unwrap().name().unwrap(), "boot_b");
        assert_eq!(gpt.get_partition_by_idx(1).unwrap().size().unwrap(), 0x3000);
    }

    /// A test helper for `read_unique_partition`
    /// It verifies that data read from partition `part` at offset `off` is the same as
    /// `part_content[off..off+sz]`.
    fn check_read_partition(
        devs: &[TestGblDisk],
        part: &str,
        part_content: &[u8],
        off: u64,
        sz: u64,
    ) {
        let mut out = vec![0u8; to_usize(sz)];
        block_on(read_unique_partition(devs, part, off, &mut out[..])).unwrap();
        assert_eq!(out, part_content[to_usize(off)..][..out.len()]);

        let mut out = vec![0u8; to_usize(sz)];
        read_unique_partition_sync(devs, part, off, &mut out[..]).unwrap();
        assert_eq!(out, part_content[to_usize(off)..][..out.len()]);
    }

    #[test]
    fn test_read_unique_partition() {
        let mut devs = FakeGblOpsStorage::default();
        devs.add_gpt_device(include_bytes!("../../libstorage/test/gpt_test_1.bin"));
        devs.add_gpt_device(include_bytes!("../../libstorage/test/gpt_test_2.bin"));
        devs.add_raw_device(c"raw_0", [0xaau8; 4 * 1024]);
        devs.add_raw_device(c"raw_1", [0x55u8; 4 * 1024]);

        let boot_a = include_bytes!("../../libstorage/test/boot_a.bin");
        let boot_b = include_bytes!("../../libstorage/test/boot_b.bin");

        let off = 512u64;
        let sz = 1024u64;
        check_read_partition(&mut devs, "boot_a", boot_a, off, sz);
        check_read_partition(&mut devs, "boot_b", boot_b, off, sz);

        let vendor_boot_a = include_bytes!("../../libstorage/test/vendor_boot_a.bin");
        let vendor_boot_b = include_bytes!("../../libstorage/test/vendor_boot_b.bin");

        check_read_partition(&mut devs, "vendor_boot_a", vendor_boot_a, off, sz);
        check_read_partition(&mut devs, "vendor_boot_b", vendor_boot_b, off, sz);

        check_read_partition(&mut devs, "raw_0", &[0xaau8; 4 * 1024][..], off, sz);
        check_read_partition(&mut devs, "raw_1", &[0x55u8; 4 * 1024][..], off, sz);
    }

    /// A test helper for `write_unique_partition`
    fn check_write_partition(devs: &[TestGblDisk], part: &str, off: u64, sz: u64) {
        // Reads the current partition content
        let (_, p) = check_part_unique(devs, part).unwrap();
        let mut part_content = vec![0u8; to_usize(p.size().unwrap())];
        block_on(read_unique_partition(devs, part, 0, &mut part_content[..])).unwrap();

        // Flips all the bits in the target range and writes back.
        let seg = &mut part_content[to_usize(off)..][..to_usize(sz)];
        seg.iter_mut().for_each(|v| *v = !(*v));
        block_on(write_unique_partition(devs, part, off, seg)).unwrap();
        // Checks that data is written.
        check_read_partition(devs, part, &part_content, off, sz);
    }

    #[test]
    fn test_write_unique_partition() {
        let mut devs = FakeGblOpsStorage::default();
        devs.add_gpt_device(include_bytes!("../../libstorage/test/gpt_test_1.bin"));
        devs.add_gpt_device(include_bytes!("../../libstorage/test/gpt_test_2.bin"));
        devs.add_raw_device(c"raw_0", [0xaau8; 4 * 1024]);
        devs.add_raw_device(c"raw_1", [0x55u8; 4 * 1024]);

        let off = 512u64;
        let sz = 1024u64;
        check_write_partition(&mut devs, "boot_a", off, sz);
        check_write_partition(&mut devs, "boot_b", off, sz);
        check_write_partition(&mut devs, "vendor_boot_a", off, sz);
        check_write_partition(&mut devs, "vendor_boot_b", off, sz);
        check_write_partition(&mut devs, "raw_0", off, sz);
        check_write_partition(&mut devs, "raw_1", off, sz);
    }

    #[test]
    fn test_rw_fail_with_non_unique_partition() {
        let mut devs = FakeGblOpsStorage::default();
        devs.add_gpt_device(include_bytes!("../../libstorage/test/gpt_test_1.bin"));
        devs.add_gpt_device(include_bytes!("../../libstorage/test/gpt_test_1.bin"));
        devs.add_raw_device(c"raw", [0xaau8; 4 * 1024]);
        devs.add_raw_device(c"raw", [0x55u8; 4 * 1024]);

        assert!(block_on(read_unique_partition(&devs, "boot_a", 0, &mut [] as &mut [u8],)).is_err());
        assert!(block_on(write_unique_partition(&devs, "boot_a", 0, &mut [],)).is_err());
        assert!(block_on(read_unique_partition(&devs, "raw", 0, &mut [] as &mut [u8],)).is_err());
        assert!(block_on(write_unique_partition(&devs, "raw", 0, &mut [],)).is_err());
    }

    #[test]
    fn test_find_multi_partition_io_fails_on_duplicate() {
        let mut devs = FakeGblOpsStorage::default();
        devs.add_raw_device(c"raw_0", [0x55u8; 4 * 1024]);
        devs.add_raw_device(c"raw_1", [0x55u8; 4 * 1024]);
        assert!(find_multi_partition_io(&devs, &["raw_0", "raw_1", "raw_0"]).is_err());
    }

    #[test]
    fn test_multi_part_io_read_fails_on_multi_part() {
        let mut devs = FakeGblOpsStorage::default();
        devs.add_raw_device(c"raw_0", [0x55u8; 4 * 1024]);
        devs.add_raw_device(c"raw_1", [0x55u8; 4 * 1024]);
        let mut io = find_multi_partition_io(&devs, &["raw_0", "raw_1"]).unwrap();
        assert!(block_on(io.read(0, &mut [0u8; 1024][..])).is_err());
    }

    #[test]
    fn test_split_partition_suffix() {
        assert_eq!(split_partition_suffix("boot_a"), Some(("boot", 'a')));
        assert_eq!(split_partition_suffix("boot_b"), Some(("boot", 'b')));
        assert_eq!(split_partition_suffix("boot__b"), Some(("boot_", 'b')));
        assert_eq!(split_partition_suffix("vendor_boot_b"), Some(("vendor_boot", 'b')));
        assert_eq!(split_partition_suffix("boo_tb"), None);
        assert_eq!(split_partition_suffix("_a"), None);
        assert_eq!(split_partition_suffix("boo"), None);
        assert_eq!(split_partition_suffix("boot_A"), None);
        assert_eq!(split_partition_suffix("boot_0"), None);
        assert_eq!(split_partition_suffix("boot_"), None);
        assert_eq!(split_partition_suffix("boot__"), None);
    }
}
