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

use crate::{
    fastboot::{BufferPool, GblFastboot, PinFutContainerTyped},
    gbl_println,
    partition::Partition,
    slots::{Bootability, Slot},
    GblOps,
};
use core::{ffi::CStr, future::Future, ops::DerefMut, str::from_utf8};
use fastboot::{next_arg, next_arg_u64, CommandResult, VarInfoSender};
use gbl_async::{block_on, select, yield_now};
use gbl_storage::BlockIo;
use liberror::Error;
use libutils::snprintf;

const MAX_DOWNLOAD_SIZE: &'static str = "max-download-size";

const VERSION_BOOTLOADER: &'static str = "version-bootloader";
const VERSION_BOOTLOADER_VAL: &'static str = "1.0";

const SLOT_COUNT: &'static str = "slot-count";
const CURRENT_SLOT: &'static str = "current-slot";
const SLOT_SUCCESSFUL: &'static str = "slot-successful";
const SLOT_UNBOOTABLE: &'static str = "slot-unbootable";
const SLOT_RETRY_COUNT: &'static str = "slot-retry-count";

const MAX_FETCH_SIZE: &'static str = "max-fetch-size";
// Limited by DATA message which only allows 8 hex digits.
// Additionally fastboot upstream parses this value as int, so we only have 31 bits.
const MAX_FETCH_SIZE_VAL: &'static str = "0x7fffffff";

const PARTITION_SIZE: &'static str = "partition-size";
const PARTITION_TYPE: &'static str = "partition-type";
const PARTITION_GUID: &'static str = "partition-guid";

const BLOCK_DEVICE: &'static str = "block-device";
const TOTAL_BLOCKS: &'static str = "total-blocks";
const BLOCK_SIZE: &'static str = "block-size";
const BLOCK_DEVICE_STATUS: &'static str = "status";

const DEFAULT_BLOCK: &'static str = "gbl-default-block";

pub(crate) const GETVAR_ALL_FILTER: &'static [&'static str] = &[
    VERSION_BOOTLOADER,
    SLOT_COUNT,
    CURRENT_SLOT,
    SLOT_SUCCESSFUL,
    SLOT_UNBOOTABLE,
    SLOT_RETRY_COUNT,
    MAX_FETCH_SIZE,
    PARTITION_SIZE,
    PARTITION_TYPE,
    PARTITION_GUID,
    BLOCK_DEVICE,
    DEFAULT_BLOCK,
    MAX_DOWNLOAD_SIZE,
];

struct FastbootSlotInfo {
    successful: &'static str,
    unbootable: &'static str,
    retry_count: usize,
}

impl From<Slot> for FastbootSlotInfo {
    fn from(slot: Slot) -> Self {
        match slot.bootability {
            Bootability::Successful => {
                FastbootSlotInfo { successful: "yes", unbootable: "no", retry_count: 0 }
            }
            Bootability::Unbootable(_) => {
                FastbootSlotInfo { successful: "no", unbootable: "yes", retry_count: 0 }
            }
            Bootability::Retriable(t) => {
                FastbootSlotInfo { successful: "no", unbootable: "no", retry_count: t.0 }
            }
        }
    }
}

// See definition of [GblFastboot] for docs on lifetimes and generics parameters.
impl<'a: 'c, 'b: 'c, 'c, 'd, 'e, G, B, S, T, P, C, F>
    GblFastboot<'a, 'b, 'c, 'd, 'e, G, B, S, T, P, C, F>
where
    G: GblOps<'a, 'e>,
    B: BlockIo,
    S: DerefMut<Target = [u8]>,
    T: DerefMut<Target = [u8]>,
    P: BufferPool,
    C: PinFutContainerTyped<'c, F>,
    F: Future<Output = ()> + 'c,
{
    /// Entry point for "fastboot getvar <variable>..."
    pub(crate) async fn get_var_internal<'s, 't>(
        &mut self,
        name: &CStr,
        args: impl Iterator<Item = &'t CStr> + Clone,
        out: &'s mut [u8],
    ) -> CommandResult<&'s str> {
        let args_str = args.clone().map(|v| v.to_str());
        // Checks that all arguments are valid str first.
        args_str.clone().find(|v| v.is_err()).unwrap_or(Ok(""))?;
        let args_str = args_str.map(|v| v.unwrap());
        Ok(match name.to_str()? {
            MAX_DOWNLOAD_SIZE => snprintf!(out, "{:#x}", self.max_download_size().await),
            VERSION_BOOTLOADER => snprintf!(out, "{}", VERSION_BOOTLOADER_VAL),
            SLOT_COUNT => snprintf!(out, "{}", self.gbl_ops.get_slot_count()?),
            CURRENT_SLOT => snprintf!(out, "{}", self.gbl_ops.get_current_slot()?.suffix.as_char()),
            SLOT_SUCCESSFUL => {
                snprintf!(out, "{}", self.get_fastboot_slot_info(args_str)?.successful)
            }
            SLOT_UNBOOTABLE => {
                snprintf!(out, "{}", self.get_fastboot_slot_info(args_str)?.unbootable)
            }
            SLOT_RETRY_COUNT => {
                snprintf!(out, "{}", self.get_fastboot_slot_info(args_str)?.retry_count)
            }
            MAX_FETCH_SIZE => snprintf!(out, "{}", MAX_FETCH_SIZE_VAL),
            PARTITION_SIZE => self.get_var_partition_size(args_str, out)?,
            PARTITION_TYPE => self.get_var_partition_type(args_str, out)?,
            PARTITION_GUID => self.get_var_partition_guid(args_str, out)?,
            BLOCK_DEVICE => self.get_var_block_device(args_str, out)?,
            DEFAULT_BLOCK => self.get_var_default_block(out)?,
            _ => {
                let sz = self.gbl_ops.fastboot_variable(name, args, out)?;
                from_utf8(out.get(..sz).ok_or("Invalid variable value size")?)?
            }
        })
    }

    /// Entry point for "fastboot getvar all..."
    pub(crate) async fn get_var_all_internal(
        &mut self,
        send: &mut impl VarInfoSender,
    ) -> CommandResult<()> {
        let mut buf = [0u8; 32];
        let dl_sz = snprintf!(buf, "{:#x}", self.max_download_size().await);
        send.send_var_info(MAX_DOWNLOAD_SIZE, [], dl_sz).await?;
        send.send_var_info(VERSION_BOOTLOADER, [], VERSION_BOOTLOADER_VAL).await?;
        match self.gbl_ops.get_slot_count() {
            Ok(slot_count) => {
                send.send_var_info(SLOT_COUNT, [], snprintf!(buf, "{slot_count}")).await?;

                let current_slot = self.gbl_ops.get_current_slot()?.suffix.as_char();
                send.send_var_info(CURRENT_SLOT, [], snprintf!(buf, "{current_slot}")).await?;

                for idx in 0..slot_count {
                    let slot = self.gbl_ops.get_slot_info(idx)?;
                    let FastbootSlotInfo { successful, unbootable, retry_count } = slot.into();
                    let mut suffix_buf = [0u8; 4];
                    let suffix = snprintf!(suffix_buf, "{}", slot.suffix.as_char());
                    send.send_var_info(SLOT_SUCCESSFUL, [suffix], successful).await?;
                    send.send_var_info(SLOT_UNBOOTABLE, [suffix], unbootable).await?;
                    send.send_var_info(SLOT_RETRY_COUNT, [suffix], snprintf!(buf, "{retry_count}"))
                        .await?;
                }
            }
            Err(Error::Unsupported | Error::NotFound) => {
                // TODO(b/442975038): Make this an error in production, allow fallback only for
                // #[cfg(feature = "gbl_dev")]
                gbl_println!(
                    self.gbl_ops,
                    "Slotting is not supported, so failed to get {SLOT_COUNT}. This will \
                    not be allowed by prod GBL soon."
                );
            }
            Err(e) => return Err(e.into()),
        }
        send.send_var_info(MAX_FETCH_SIZE, [], MAX_FETCH_SIZE_VAL).await?;
        self.get_all_block_device(send).await?;
        send.send_var_info(DEFAULT_BLOCK, [], self.get_var_default_block(&mut buf)?).await?;
        self.get_all_partition_size_type(send).await?;

        // Gets platform specific variables
        let tasks = self.tasks();
        Ok(self.gbl_ops.fastboot_visit_all_variables(|ops, args, val| {
            if let Some((name, args)) = args.split_first_chunk::<1>() {
                let name = name[0].to_str().unwrap_or("?");
                // Needs to split because the interface allows backend to pass ':' concatenated
                // string as a whole.
                let var = name.split(':').next().unwrap_or(name);
                if GETVAR_ALL_FILTER.iter().find(|v| **v == var).is_some() {
                    // Its possible that backend might have its own special non-gpt/raw block
                    // partitions (i.e. virtual) and therefore needs to expose
                    // `partition-size/partition-type` vars for them. If this will be the case,
                    // allow `partition-size/partition-type` when partition doesn't exist.
                    gbl_println!(ops, "Variable {var:?} is reserved by GBL.");
                    return;
                }
                let args = args.iter().map(|v| v.to_str().unwrap_or("?"));
                let val = val.to_str().unwrap_or("?");
                // Manually polls async tasks so that we can still get parallelism while running in
                // the context of backend.
                let _ = block_on(select(send.send_var_info(name, args, val), async {
                    loop {
                        tasks.borrow_mut().poll_all();
                        yield_now().await;
                    }
                }));
            }
        })?)
    }

    /// Gets the max-download-size variable.
    pub(crate) async fn max_download_size(&mut self) -> usize {
        self.get_download_buffer().await.len()
    }

    /// Parses and finds the size of the given partition.
    pub(crate) fn partition_size<'s>(&mut self, part: &'s str) -> CommandResult<u64> {
        let (part, blk_id, off, sz) = self.parse_partition_arg(part)?;
        let parts = self.resolve_slotted_partitions(part, blk_id)?;
        // Slotted partition-size only make sense if they are all the same. Because it may be used
        // to copy AVB footer.
        for (_, p) in &parts[1..] {
            if p.size()? != parts[0].1.size()? {
                return Err(format_args!(
                    "{} and {} has different partition sizes",
                    p.name().unwrap_or("?"),
                    parts[0].1.name().unwrap_or("?")
                )
                .into());
            }
        }
        let (start, end) = parts[0].1.sub(off, sz)?;
        Ok(end - start)
    }

    /// "fastboot getvar partition-size"
    fn get_var_partition_size<'s, 't>(
        &mut self,
        mut args: impl Iterator<Item = &'t str> + Clone,
        out: &'s mut [u8],
    ) -> CommandResult<&'s str> {
        Ok(snprintf!(out, "{:#x}", self.partition_size(args.next().ok_or("Missing partition")?)?))
    }

    /// "fastboot getvar partition-type"
    fn get_var_partition_type<'s, 't>(
        &mut self,
        mut args: impl Iterator<Item = &'t str> + Clone,
        out: &'s mut [u8],
    ) -> CommandResult<&'s str> {
        self.partition_size(args.next().ok_or("Missing partition")?)?;
        Ok(snprintf!(out, "raw"))
    }

    /// "fastboot getvar partition-guid"
    fn get_var_partition_guid<'s, 't>(
        &mut self,
        mut args: impl Iterator<Item = &'t str> + Clone,
        out: &'s mut [u8],
    ) -> CommandResult<&'s str> {
        let (part, blk_id, _, _) =
            self.parse_partition_arg(args.next().ok_or("Missing partition")?)?;
        let (_, ptn) = self.find_partition(part, blk_id)?;

        match ptn {
            Partition::Gpt(gpt_partition) => {
                Ok(snprintf!(out, "{}", gpt_partition.gpt_entry().guid))
            }
            _ => Err("Not a GPT partition".into()),
        }
    }

    /// Gets all "partition-size/partition-type"
    async fn get_all_partition_size_type(
        &mut self,
        responder: &mut impl VarInfoSender,
    ) -> CommandResult<()> {
        // Though any sub range of a GPT partition or raw block counts as a partition in GBL
        // Fastboot, for "getvar all" we only enumerate whole range GPT partitions.
        let disks = self.disks;
        // Allocates 36 bytes to fit partition GUID.
        let mut size_str = [0u8; 36];
        for (idx, blk) in disks.iter().enumerate() {
            for ptn_idx in 0..blk.num_partitions().unwrap_or(0) {
                let ptn = blk.get_partition_by_idx(ptn_idx)?;
                let sz: u64 = ptn.size()?;
                let part = ptn.name()?;
                // Assumes max partition name length of 72 plus max u64 hex string length 18.
                let mut part_id_buf = [0u8; 128];
                // If partition is not unique, append block ID suffix.
                let part = match crate::partition::check_part_unique(disks, part) {
                    Ok(_) => snprintf!(part_id_buf, "{}", part),
                    Err(_) => snprintf!(part_id_buf, "{}/{:x}", part, idx),
                };
                responder
                    .send_var_info(PARTITION_SIZE, [part], snprintf!(size_str, "{:#x}", sz))
                    .await?;
                // Image type is not supported yet.
                responder.send_var_info(PARTITION_TYPE, [part], snprintf!(size_str, "raw")).await?;
                if let Partition::Gpt(gpt_partition) = ptn {
                    let guid = gpt_partition.gpt_entry().guid;
                    responder
                        .send_var_info(PARTITION_GUID, [part], snprintf!(size_str, "{}", guid))
                        .await?;
                }
            }
        }
        Ok(())
    }

    /// Block device related information.
    ///
    /// `fastboot getvar block-device:<id>:total-blocks`
    /// `fastboot getvar block-device:<id>:block-size`
    /// `fastboot getvar block-device:<id>:status`
    fn get_var_block_device<'s, 't>(
        &mut self,
        mut args: impl Iterator<Item = &'t str> + Clone,
        out: &'s mut [u8],
    ) -> CommandResult<&'s str> {
        let id = next_arg_u64(&mut args)?.ok_or("Missing block device ID")?;
        let id = usize::try_from(id)?;
        let val_type = next_arg(&mut args).ok_or("Missing value type")?;
        let blk = &self.disks[id];
        let info = blk.block_info();
        Ok(match val_type {
            TOTAL_BLOCKS => snprintf!(out, "{:#x}", info.num_blocks),
            BLOCK_SIZE => snprintf!(out, "{:#x}", info.block_size),
            BLOCK_DEVICE_STATUS => {
                snprintf!(out, "{}", blk.status().to_str())
            }
            _ => return Err("Invalid type".into()),
        })
    }

    /// Gets all "block-device" variables.
    async fn get_all_block_device(
        &mut self,
        responder: &mut impl VarInfoSender,
    ) -> CommandResult<()> {
        let mut val = [0u8; 32];
        for (idx, blk) in self.gbl_ops.disks().iter().enumerate() {
            let mut id_str = [0u8; 32];
            let id = snprintf!(id_str, "{:x}", idx);
            let info = blk.block_info();
            responder
                .send_var_info(
                    BLOCK_DEVICE,
                    [id, TOTAL_BLOCKS],
                    snprintf!(val, "{:#x}", info.num_blocks),
                )
                .await?;
            responder
                .send_var_info(
                    BLOCK_DEVICE,
                    [id, BLOCK_SIZE],
                    snprintf!(val, "{:#x}", info.block_size),
                )
                .await?;
            responder
                .send_var_info(
                    BLOCK_DEVICE,
                    [id, BLOCK_DEVICE_STATUS],
                    snprintf!(val, "{}", blk.status().to_str()),
                )
                .await?;
        }
        Ok(())
    }

    /// "fastboot getvar gbl-default-block"
    fn get_var_default_block<'s>(&mut self, out: &'s mut [u8]) -> CommandResult<&'s str> {
        Ok(match self.default_block {
            Some(v) => snprintf!(out, "{:#x}", v),
            None => snprintf!(out, "None"),
        })
    }

    /// "fastboot getvar slot-successful:<slot-suffix>"
    /// "fastboot getvar slot-unbootable:<slot-suffix>"
    /// "fastboot getvar slot-retry-count:<slot-suffix>"
    fn get_fastboot_slot_info<'t>(
        &mut self,
        mut args: impl Iterator<Item = &'t str> + Clone,
    ) -> CommandResult<FastbootSlotInfo> {
        let suffix_str = args.next().ok_or("Missing slot suffix")?;
        if suffix_str.chars().count() != 1 {
            return Err("Slot suffix must be a single character".into());
        }
        let suffix = suffix_str.chars().next().ok_or("Invalid slot")?;
        let slot = self
            .slots_iter()?
            .find(|slot| slot.is_ok() && slot.unwrap().suffix.as_char() == suffix)
            .ok_or("Invalid slot")??;
        Ok(slot.into())
    }
}
