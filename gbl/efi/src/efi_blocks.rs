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

use alloc::vec::Vec;
use core::cmp::max;
use efi::{
    efi_println,
    profiling::EfiProfileBackend,
    protocol::{block_io::BlockIoProtocol, block_io2::BlockIo2Protocol, Protocol},
    EfiEntry,
};
use efi_types::EfiBlockIoMedia;
use gbl_async::block_on;
use gbl_storage::{gpt_buffer_size, BlockInfo, BlockIo, Disk, Gpt, SliceMaybeUninit};
use liberror::Error;
use libgbl::partition::GblDisk;
use libprofile_macros::profile;

/// `EfiBlockDeviceIo` wraps a EFI `BlockIoProtocol` and optionally a `BlockIo2Protocol` and
/// implements the `BlockIo` interface.
///
/// `BlockIoProtocol` is always required and used for implementation of `read_blocks_sync` and
/// `write_blocks_sync``. When `BlockIo2Protocol` is provided, it will be used to implement
/// `read_blocks` and `write_blocks`, otherwise they fall back to `BlockIoProtocol`.
pub struct EfiBlockDeviceIo<'a> {
    block_io: Protocol<'a, BlockIoProtocol>,
    block_io2: Option<Protocol<'a, BlockIo2Protocol>>,
}

impl<'a> EfiBlockDeviceIo<'a> {
    fn media(&self) -> EfiBlockIoMedia {
        self.block_io.media().unwrap()
    }

    fn info(&mut self) -> BlockInfo {
        let media = self.media();
        BlockInfo {
            block_size: media.block_size as u64,
            num_blocks: (media.last_block + 1) as u64,
            alignment: max(1, media.io_align as u64),
        }
    }
}

// SAFETY:
// `read_blocks()` usess EFI protocol that guarantees to read exact number of blocks that were
// requested, or return error.
// For async `read_blocks_ex()` blocking wait guarantees that read finishes.
unsafe impl BlockIo for EfiBlockDeviceIo<'_> {
    fn info(&mut self) -> BlockInfo {
        (*self).info()
    }

    async fn read_blocks(
        &mut self,
        blk_offset: u64,
        out: &mut (impl SliceMaybeUninit + ?Sized),
    ) -> Result<(), Error> {
        match &self.block_io2 {
            Some(v) => v.read_blocks_ex(blk_offset, out).await,
            _ => self.block_io.read_blocks(blk_offset, out),
        }
        .or(Err(Error::BlockIoError))
    }

    async fn write_blocks(&mut self, blk_offset: u64, data: &mut [u8]) -> Result<(), Error> {
        match &self.block_io2 {
            Some(v) => v.write_blocks_ex(blk_offset, data).await,
            _ => self.block_io.write_blocks(blk_offset, data),
        }
        .or(Err(Error::BlockIoError))
    }

    fn read_blocks_sync(
        &mut self,
        blk_offset: u64,
        out: &mut (impl SliceMaybeUninit + ?Sized),
    ) -> Result<(), Error> {
        self.block_io.read_blocks(blk_offset, out).or(Err(Error::BlockIoError))
    }

    fn write_blocks_sync(&mut self, blk_offset: u64, data: &mut [u8]) -> Result<(), Error> {
        self.block_io.write_blocks(blk_offset, data).or(Err(Error::BlockIoError))
    }
}

const MAX_GPT_ENTRIES: usize = 128;

/// The [GblDisk] type in the GBL EFI context.
pub type EfiGblDisk<'a> = GblDisk<Disk<EfiBlockDeviceIo<'a>, Vec<u8>>, Gpt<Vec<u8>>>;

/// Finds and returns all EFI devices supporting either EFI_BLOCK_IO or EFI_BLOCK_IO2 protocol.
#[profile(backend = EfiProfileBackend::new(efi_entry))]
pub fn find_block_devices(efi_entry: &EfiEntry) -> Result<Vec<EfiGblDisk<'_>>, Error> {
    let bs = efi_entry.system_table().boot_services();
    let block_dev_handles = bs.locate_handle_buffer_by_protocol::<BlockIoProtocol>()?;
    let mut gbl_disks = vec![];
    let gpt_buffer_size = gpt_buffer_size(MAX_GPT_ENTRIES)?;
    for (idx, handle) in block_dev_handles.handles().iter().enumerate() {
        let block_io = bs.open_protocol::<BlockIoProtocol>(*handle).unwrap();
        let block_io2 = bs.open_protocol::<BlockIo2Protocol>(*handle).ok();
        let blk_io = EfiBlockDeviceIo { block_io, block_io2 };
        if blk_io.media().logical_partition {
            continue;
        }
        // TODO(b/357688291): Support raw partition based on device path info.
        let disk = GblDisk::new_gpt(
            Disk::new_alloc_scratch(blk_io).unwrap(),
            Gpt::new(vec![0u8; gpt_buffer_size]).unwrap(),
        );
        match block_on(disk.as_sync().unwrap().sync_gpt()) {
            Ok(Some(v)) => efi_println!(efi_entry, "Block #{idx} GPT sync result: {v}"),
            Err(e) => efi_println!(efi_entry, "Block #{idx} error while syncing GPT: {e}"),
            _ => {}
        };
        gbl_disks.push(disk);
    }
    Ok(gbl_disks)
}
