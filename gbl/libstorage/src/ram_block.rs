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

//! This module provides an implementation of [BlockIo] backed by RAM.

use crate::{is_aligned, is_buffer_aligned, BlockInfo, BlockIo};
use bytes::buf::UninitSlice;
use core::ops::DerefMut;
use gbl_async::yield_now;
use liberror::Error;
use safemath::SafeNum;

/// `RamBlockIo` implements [BlockIo] backed by user provided buffer.
pub struct RamBlockIo<T> {
    /// The storage block size in bytes.
    pub block_size: u64,
    /// The storage access alignment in bytes.
    pub alignment: u64,
    /// The backing storage data.
    pub storage: T,
    /// The number of successful write calls.
    pub num_writes: usize,
    /// The number of successful read calls.
    pub num_reads: usize,
    /// Injected error to be returned by the next read/write/erase IO.
    pub error: Option<Error>,
}

impl<T: DerefMut<Target = [u8]>> RamBlockIo<T> {
    /// Creates a new instance.
    pub fn new(block_size: u64, alignment: u64, storage: T) -> Self {
        assert_eq!(
            storage.len() % usize::try_from(block_size).unwrap(),
            0,
            "storage size is not multiple of block size, {}, {}",
            storage.len(),
            block_size
        );
        Self { block_size, alignment, storage, num_writes: 0, num_reads: 0, error: None }
    }

    /// Gets the underlying ramdisk storage.
    pub fn storage(&mut self) -> &mut [u8] {
        &mut self.storage[..]
    }

    /// Helper for checking custom injected errors
    fn check_custom_error(&mut self) -> Result<(), Error> {
        match self.error.take() {
            Some(e) => Err(e),
            _ => Ok(()),
        }
    }

    /// Checks injected error, simulates async waiting, checks read/write parameters and returns the
    /// offset in number of bytes.
    async fn checks<'a>(
        &mut self,
        blk_offset: u64,
        buf: impl Into<&'a mut UninitSlice>,
    ) -> Result<usize, Error> {
        let buf = buf.into();
        assert!(is_buffer_aligned(&mut *buf, self.alignment).unwrap_or(false));
        assert!(is_aligned(buf.len(), self.block_size).unwrap_or(false));
        yield_now().await;
        self.check_custom_error()?;
        Ok((SafeNum::from(blk_offset) * self.block_size).try_into().unwrap())
    }
}

// SAFETY:
// `read_blocks` clones `out.len()` bytes to output which initializes all elements in `out`
unsafe impl<T: DerefMut<Target = [u8]>> BlockIo for RamBlockIo<T> {
    fn info(&self) -> BlockInfo {
        BlockInfo {
            block_size: self.block_size,
            erase_blocks_num: 2,
            num_blocks: u64::try_from(self.storage.len()).unwrap() / self.block_size,
            alignment: self.alignment,
        }
    }

    async fn read_blocks<'a>(
        &mut self,
        blk_offset: u64,
        out: impl Into<&'a mut UninitSlice>,
    ) -> Result<(), Error> {
        let out = out.into();
        let offset = self.checks(blk_offset, &mut *out).await?;
        let out_len = out.len();
        Ok(out.copy_from_slice(&self.storage[offset..][..out_len]))
    }

    async fn write_blocks(&mut self, blk_offset: u64, data: &mut [u8]) -> Result<(), Error> {
        let offset = self.checks(blk_offset, &mut *data).await?;
        Ok(self.storage[offset..][..data.len()].copy_from_slice(data))
    }

    async fn erase_blocks(&mut self, blk_offset: u64, num_blks: u64) -> Result<(), Error> {
        yield_now().await;
        self.check_custom_error()?;
        let blk_sz = self.info().erase_block_size().unwrap();
        let off = (SafeNum::from(blk_offset) * blk_sz).try_into().unwrap();
        let sz = (SafeNum::from(num_blks) * blk_sz).try_into().unwrap();
        // Erases by flipping the bits.
        Ok(self.storage[off..][..sz].iter_mut().for_each(|v| *v = !*v))
    }
}
