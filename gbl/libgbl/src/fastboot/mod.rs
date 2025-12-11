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

//! Fastboot backend for libgbl.

#[cfg(feature = "fuchsia")]
use crate::{
    android_boot::get_kernel,
    fuchsia_boot::{
        zbi_split_unused_buffer_ref, zircon_load_verify_abr_with_buffer, GblAbrOps,
        LoadedVerifiedZircon,
    },
};
use crate::{
    android_boot::{
        android_load_verify_fixup, get_boot_slot, load::sub_slice_range, BootBuffer, LoadedImages,
    },
    gbl_println,
    misc::{read_bootloader_message_to, write_bootloader_message, AndroidBootMode},
    ops::{CommandExecType, FastbootEraseAction, RambootOps},
    partition::{check_part_unique, GblDisk, MultiPartitionIo, Partition, PartitionIo, RawName},
    slots::Slot,
    GblOps, IntegrationError,
};
pub use abr::{mark_slot_active, set_one_shot_bootloader, set_one_shot_recovery, SlotIndex};
use arrayvec::ArrayVec;
use core::{
    array::from_fn,
    cell::RefCell,
    cmp::min,
    ffi::CStr,
    fmt::{Display, Write},
    future::Future,
    marker::PhantomData,
    mem::take,
    ops::{DerefMut, Range},
    pin::Pin,
    str::from_utf8,
};
use fastboot::{
    next_arg, next_arg_u64, process_next_command, run_tcp_session, CommandError, CommandResult,
    DownloadBuilder, Downloader, FailSender, FastbootImplementation, InfoSender, LockState,
    LockType, OkaySender, RebootMode, Unlockability, UploadBuilder, Uploader, VarInfoSender,
    MAX_COMMAND_SIZE,
};
use gbl_async::{join, yield_now};
use gbl_storage::{BlockIo, Disk, Gpt};
use liberror::Error;
use libutils::snprintf;
use libutils::FormattedBytes;
use safemath::SafeNum;
#[cfg(feature = "fuchsia")]
use zbi::{ZbiContainer, ZbiType};

pub(crate) mod vars;

pub(crate) mod sparse;
use sparse::is_sparse_image;

mod shared;
pub use shared::Shared;

mod buffer_pool;
pub use buffer_pool::BufferPool;
use buffer_pool::ScopedBuffer;

mod pin_fut_container;
pub use pin_fut_container::PinFutContainer;
use pin_fut_container::{PinFutContainerTyped, PinFutSlice};

// Re-exports dependency types
pub use fastboot::{TcpStream, Transport};

pub(crate) mod boot_items;
use boot_items::{BootItem, BootItemContainer};

/// Reserved name for indicating flashing GPT.
const FLASH_GPT_PART: &str = "gpt";

/// Maximum number of partitions for `MultiPartitionIo` in the fastboot context.
/// This is determined by the maximum number slots supported.
const MAX_IO_PARTS: usize = 2;

/// Represents the workload of a GBL Fastboot async task.
enum TaskWorkload<'a, 'b, B: BlockIo, P: BufferPool> {
    /// Image flashing task. (partition io, downloaded data, data size)
    Flash(MultiPartitionIo<'a, B, MAX_IO_PARTS>, ScopedBuffer<'b, P>, usize),
    /// Sparse image flashing task. (partition io, downloaded data)
    FlashSparse(MultiPartitionIo<'a, B, MAX_IO_PARTS>, ScopedBuffer<'b, P>),
    // Image erase task.
    Erase(MultiPartitionIo<'a, B, MAX_IO_PARTS>, ScopedBuffer<'b, P>),
    None,
}

impl<'a, 'b, B: BlockIo, P: BufferPool> TaskWorkload<'a, 'b, B, P> {
    /// Runs the task and returns the result
    async fn run(self) -> Result<(), Error> {
        match self {
            Self::Flash(mut io, mut data, sz) => io.write(0, &mut data[..sz]).await,
            Self::FlashSparse(mut io, mut data) => io.write_sparse(0, &mut data).await,
            Self::Erase(mut io, mut buffer) => match io.erase(&mut buffer).await {
                Err(Error::Unsupported) => io.zeroize(&mut buffer).await,
                v => v,
            },
            _ => Ok(()),
        }
    }
}

/// Represents a GBL Fastboot async task.
struct Task<'a, 'b, B: BlockIo, P: BufferPool> {
    workload: TaskWorkload<'a, 'b, B, P>,
    context: [u8; MAX_COMMAND_SIZE],
}

impl<'a, 'b, B: BlockIo, P: BufferPool> Task<'a, 'b, B, P> {
    /// Creates a new instance with the given workload.
    fn new(workload: TaskWorkload<'a, 'b, B, P>) -> Self {
        Self { workload, context: [0u8; MAX_COMMAND_SIZE] }
    }

    /// Sets the context string.
    fn set_context(&mut self, mut f: impl FnMut(&mut dyn Write) -> Result<(), core::fmt::Error>) {
        let _ = f(&mut FormattedBytes::new(&mut self.context[..]));
    }

    /// Runs the task and returns the result.
    async fn run_checked(self) -> Result<(), Error> {
        self.workload.run().await
    }

    /// Runs the task. Panics on error.
    ///
    /// The method is intended for use in the context of parallel/background async task where errors
    /// can't be easily handled by the main routine.
    async fn run(self) {
        match self.workload.run().await {
            Err(e) => panic!(
                "A Fastboot async task failed: {e:?}, context: {}",
                from_utf8(&self.context[..]).unwrap_or("")
            ),
            _ => {}
        }
    }
}

impl<'a, 'b, B: BlockIo, P: BufferPool> Default for Task<'a, 'b, B, P> {
    fn default() -> Self {
        // Creates a noop task. This is mainly used for type inference for inline declaration of
        // pre-allocated task pool.
        Self::new(TaskWorkload::None)
    }
}

/// Contains the load buffer layout of images loaded by "fastboot boot".
#[derive(Clone, Debug, PartialEq, Default)]
pub enum LoadedImageInfo {
    /// None
    #[default]
    None,
    /// Android loaded images.
    Android {
        /// Address range of ramdisk.
        ramdisk: Range<*const u8>,
        /// Address range of fdt.
        fdt: Range<*const u8>,
        /// Address range of kernel.
        kernel: Range<*const u8>,
    },
    /// Fuchsia loaded images.
    #[cfg(feature = "fuchsia")]
    Fuchsia {
        /// Offset and length of ZBI items in `GblFastboot::load_buffer`.
        zbi_items: Range<*const u8>,
        /// Offset and length of kernel in `GblFastboot::load_buffer`.
        kernel: Range<*const u8>,
        /// Selected slot,
        slot: SlotIndex,
    },
}

/// Helper function for splitting loaded android images from a boot buffer.
pub fn split_loaded_android<'a>(
    info: LoadedImageInfo,
    mut boot_buffer: BootBuffer<'a>,
) -> Option<(&'a [u8], &'a [u8], &'a [u8], &'a mut [u8])> {
    let LoadedImageInfo::Android { ramdisk, fdt, kernel } = &info else {
        return None;
    };

    // Computes the size of each image component.
    let [ramdisk_sz, fdt_sz, kernel_sz] = [&ramdisk, &fdt, &kernel].map(|v| ptr_range_len(v));

    // Partitions the general load buffer.
    let general_buf = boot_buffer.take_boot_items().split_unused();
    let general = &general_buf.as_ptr_range();
    let ramdisk = sub_slice_range(general, ramdisk).unwrap_or(0..0);
    let fdt = sub_slice_range(general, fdt).unwrap_or(ramdisk.end..ramdisk.end);
    let kernel = sub_slice_range(general, kernel).unwrap_or(fdt.end..fdt.end);
    assert!(fdt.start >= ramdisk.end && kernel.start >= fdt.end);
    let (rem, unused) = general_buf.split_at_mut(kernel.end);
    let (rem, general_kernel) = rem.split_at_mut(kernel.start);
    let (rem, general_fdt) = rem.split_at_mut(fdt.start);
    let (_, general_ramdisk) = rem.split_at_mut(ramdisk.start);

    // Chooses between designated or partitioned general buffer.
    let ramdisk = &mut boot_buffer.ramdisk.unwrap_or(general_ramdisk)[..ramdisk_sz];
    let fdt = &mut boot_buffer.fdt.unwrap_or(general_fdt)[..fdt_sz];
    let kernel = &mut boot_buffer.kernel.unwrap_or(general_kernel)[..kernel_sz];
    Some((ramdisk, fdt, kernel, unused))
}

/// Contains result data returned by GBL Fastboot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GblFastbootResult {
    /// Buffer layout for images loaded by "fastboot boot"
    pub loaded_image_info: LoadedImageInfo,
    /// Slot suffix that was last set active by "fastboot set_active"
    pub last_set_active_slot: Option<char>,
    /// Whether to stop in fastboot mode after load verify fixup.
    pub pause_in_fastboot: bool,
}

impl GblFastbootResult {
    /// Splits the given buffer into `(ramdisk, fdt, kernel, unused)` according to layout info in
    ///  `Self::loaded_image_info` if it is a `Some(LoadedImageInfo::Android)`.
    pub(crate) fn split_loaded_android<'a>(
        &self,
        boot_buffer: BootBuffer<'a>,
    ) -> Option<(&'a [u8], &'a [u8], &'a [u8], &'a mut [u8])> {
        split_loaded_android(self.loaded_image_info.clone(), boot_buffer)
    }

    /// Splits the given buffer into `(zbi_items, kernel)` according to layout info in
    /// `Self::loaded_image_info` if it is a `Some(LoadedImageInfo::Fuchsia)`. `load` should be the
    /// same buffer passed to GblFastboot.
    #[cfg(feature = "fuchsia")]
    pub(crate) fn split_loaded_fuchsia<'a>(
        &self,
        load: &'a mut [u8],
    ) -> Option<(&'a mut [u8], &'a mut [u8])> {
        let LoadedImageInfo::Fuchsia { zbi_items, kernel, .. } = &self.loaded_image_info else {
            return None;
        };
        let zbi_items = sub_slice_range(&load.as_ptr_range(), zbi_items).unwrap();
        let kernel = sub_slice_range(&load.as_ptr_range(), kernel).unwrap();
        let (zbi_items_buf, rem) = load[zbi_items.start..].split_at_mut(zbi_items.len());
        let (kernel_buf, _) = rem[kernel.start - zbi_items.end..].split_at_mut(kernel.len());
        Some((zbi_items_buf, kernel_buf))
    }
}

/// Helper for computing the length of a pointer range.
fn ptr_range_len(range: &Range<*const u8>) -> usize {
    (range.end as usize).checked_sub(range.start as _).unwrap()
}

// Represents GBL specific stage data type
#[derive(Copy, Clone, Debug)]
enum StageDataType {
    LoadedKernel,
    LoadedRamdisk,
    LoadedFdt,
}

#[derive(Default)]
pub(crate) struct GblFbData<'a> {
    pub(crate) boot_buffer: BootBuffer<'a>,
    pub(crate) load_result: Option<Result<LoadedImages<'a>, &'a IntegrationError>>,
}

/// `GblFastboot` implements fastboot commands in the GBL context.
///
/// # Lifetimes
///
/// * `'a`: [GblOps] and disks lifetime.
/// * `'b`: Lifetime for the buffer allocated by `P`.
/// * `'c`: Lifetime of the pinned [Future]s in task container `task`.
/// * `'d`: Lifetime of the `tasks` and `gbl_ops` objects borrowed.
///
/// # Generics
///
/// * `G`: Type of `Self::gbl_ops` which implements [GblOps].
/// * `B`: Type that implements [BlockIo] in the [Disk] parameter of [GblDisk] for `Self::disks`.
/// * `S`: Type of scratch buffer in the [Disk] parameter of [GblDisk] for `Self::disks`.
/// * `T`: Type of gpt buffer in the [Gpt] parameter of [GblDisk] for `Self::disks`.
/// * `P`: Type of `Self::buffer_pool` which implements [BufferPool].
/// * `C`: Type of `Self::tasks` which implements [PinFutContainerTyped].
/// * `F`: Type of [Future] stored by `Self::Tasks`.
struct GblFastboot<'a, 'b, 'c, 'd, G, B, S, T, P, C, F>
where
    G: GblOps<'a>,
    B: BlockIo,
    S: DerefMut<Target = [u8]>,
    T: DerefMut<Target = [u8]>,
    P: BufferPool,
{
    pub(crate) gbl_ops: &'d mut G,
    // We store the partition devices returned by `gbl_ops.disks()` directly instead of getting it
    // from `gbl_ops` later because we need to establish to the compiler that the hidden type of
    // [BlockIo] in `GblDisk<Disk<impl BlockIO...>...>` returned by `gbl_ops.disks()` will be the
    // same as the [BlockIo] type (denoted as B) in the function pointer
    // `task_mapper`: fn(Task<'a, 'b, B, P>) -> F`. Otherwise, compiler won't allow `fn flash()`
    // to call `task_mapper` with a `Task` constructed from `GblDisk<Disk<impl BlockIO...>...>`.
    disks: &'a [GblDisk<Disk<B, S>, Gpt<T>>],
    buffer_pool: &'b Shared<P>,
    task_mapper: fn(Task<'a, 'b, B, P>) -> F,
    tasks: &'d Shared<C>,
    current_download_buffer: Option<ScopedBuffer<'b, P>>,
    current_download_size: usize,
    enable_async_task: bool,
    expected_download_crc: Option<u32>,
    default_block: Option<usize>,
    data: GblFbData<'b>,
    stage_data_type: Option<StageDataType>,
    result: GblFastbootResult,
    // Introduces marker type so that we can enforce constraint 'd <= min('b, 'c).
    // The constraint is expressed in the implementation block for the `FastbootImplementation`
    // trait.
    _tasks_context_lifetime: PhantomData<&'c P>,
}

// See definition of [GblFastboot] for docs on lifetimes and generics parameters.
impl<'a: 'c, 'b: 'c, 'c, 'd, G, B, S, T, P, C, F> GblFastboot<'a, 'b, 'c, 'd, G, B, S, T, P, C, F>
where
    G: GblOps<'a>,
    B: BlockIo,
    S: DerefMut<Target = [u8]>,
    T: DerefMut<Target = [u8]>,
    P: BufferPool,
    C: PinFutContainerTyped<'c, F>,
    F: Future<Output = ()> + 'c,
{
    /// Creates a new [GblFastboot].
    ///
    /// # Args
    ///
    /// * `gbl_ops`: An implementation of `GblOps`.
    /// * `disks`: The disk devices returned by `gbl_ops.disks()`. This is needed for expressing the
    ///   property that the hidden [BlockIo] type is the same as that in `task_mapper`.
    /// * `task_mapper`: A function pointer that maps `Task<'a, 'b, G, B>` to the target [Future]
    ///   type `F` for input to `PinFutContainerTyped<F>::add_with()`.
    /// * `tasks`: A shared instance of `PinFutContainerTyped<F>`.
    /// * `buffer_pool`: A shared instance of `BufferPool`.
    /// * `data`: Additional data provided to fastboot.
    ///
    /// The combination of `task_mapper` and `tasks` allows type `F`, which will be running the
    /// async function `Task::run()`, to be defined at the callsite. This is necessary for the
    /// usage of preallocated pinned futures (by `run_gbl_fastboot_stack()`) because the returned
    /// type of a `async fn` is compiler-generated and can't be named. The only way to create a
    /// preallocated slice of anonymous future is to keep the type generic and pass in the
    /// anonymous future instance at the initialization callsite (aka defining use) and let compiler
    /// infer and propagate it.
    fn new(
        gbl_ops: &'d mut G,
        disks: &'a [GblDisk<Disk<B, S>, Gpt<T>>],
        task_mapper: fn(Task<'a, 'b, B, P>) -> F,
        tasks: &'d Shared<C>,
        buffer_pool: &'b Shared<P>,
        data: GblFbData<'b>,
    ) -> Self {
        Self {
            gbl_ops,
            disks,
            task_mapper,
            tasks,
            buffer_pool,
            current_download_buffer: None,
            current_download_size: 0,
            enable_async_task: false,
            expected_download_crc: None,
            default_block: None,
            data,
            stage_data_type: None,
            result: Default::default(),
            _tasks_context_lifetime: PhantomData,
        }
    }

    /// Returns the shared task container.
    // Rust edition 2024 by default catpures all lifetimes, which is unnecessarily strict. Thus use
    // explicit `use` capture. Rust requires all type parameters be added in use.
    fn tasks(&self) -> &'d Shared<impl PinFutContainerTyped<'c, F> + use<'c, G, B, S, T, P, C, F>> {
        self.tasks
    }

    /// Listens on the given transports and TCP channels and runs fastboot.
    async fn run(
        &mut self,
        transports: &mut [impl GblGenericTransport],
        mut tcp: Option<impl GblTcpStream>,
    ) {
        if transports.is_empty() && tcp.is_none() {
            gbl_println!(self.gbl_ops, "No transports found for GBL Fastboot");
            return;
        }
        let tasks = self.tasks();
        // The fastboot command loop task for interacting with the remote host.
        let cmd_loop_end = Shared::from(false);

        // This is main loop for processing all transports one by one.
        // `process_next_command()` may be calling `receive()` multiple times.
        // This may delay processing of other protocols.
        //
        // E.g.
        // If any transport protocol perform long operation like `flash`.
        // It would delay calls to transport implementation responsible for UI.
        let cmd_loop_task = async {
            'outer: loop {
                for t in transports.iter_mut() {
                    if t.has_packet() {
                        match process_next_command(t, self).await {
                            Ok(true) => break 'outer,
                            Err(e) => {
                                gbl_println!(self.gbl_ops, "GBL Fastboot transport error: {e}")
                            }
                            _ => (),
                        };
                    }
                }

                if let Some(v) = tcp.as_mut() {
                    if v.accept_new() {
                        match run_tcp_session(v, self).await {
                            Ok(()) => break 'outer,
                            Err(e) if e != Error::Disconnected => {
                                gbl_println!(self.gbl_ops, "GBL Fastboot TCP session error: {e}");
                            }
                            _ => (),
                        };
                    }
                }

                yield_now().await;
            }
            *cmd_loop_end.borrow_mut() = true;
        };

        // Schedules [Task] spawned by GBL fastboot.
        let gbl_fb_tasks = async {
            while tasks.borrow_mut().poll_all() > 0 || !*cmd_loop_end.borrow_mut() {
                yield_now().await;
            }
        };

        let _ = join(cmd_loop_task, gbl_fb_tasks).await;
    }

    /// Extracts the next argument and verifies that it is a valid block device ID if present.
    ///
    /// # Returns
    ///
    /// * Returns `Ok(Some(blk_id))` if next argument is present and is a valid block device ID.
    /// * Returns `None` if next argument is not available and there are more than one block
    ///   devices.
    /// * Returns `Err(())` if next argument is present but is an invalid block device ID.
    fn check_next_arg_blk_id<'s>(
        &self,
        args: &mut impl Iterator<Item = &'s str>,
    ) -> CommandResult<Option<usize>> {
        let devs = self.disks;
        let blk_id = match next_arg_u64(args)? {
            Some(v) => {
                let v = usize::try_from(v)?;
                // Checks out of range.
                devs.get(v).ok_or("Invalid block ID")?;
                Some(v)
            }
            _ => None,
        };
        let blk_id = blk_id.or(self.default_block);
        let blk_id = blk_id.or((devs.len() == 1).then_some(0));
        Ok(blk_id)
    }

    /// Parses and checks the argument for "fastboot flash gpt/<blk_idx>/"resize".
    ///
    /// # Returns
    ///
    /// * Returns `Ok(Some((blk_idx, resize)))` if command is a GPT flashing command.
    /// * Returns `Ok(None)` if command is not a GPT flashing command.
    /// * Returns `Err()` otherwise.
    pub(crate) fn parse_flash_gpt_args(&self, part: &str) -> CommandResult<Option<(usize, bool)>> {
        // Syntax: flash gpt/<blk_idx>/"resize"
        let mut args = part.split('/');
        if next_arg(&mut args).filter(|v| *v == FLASH_GPT_PART).is_none() {
            return Ok(None);
        }
        // Parses block device ID.
        let blk_id = self
            .check_next_arg_blk_id(&mut args)?
            .ok_or("Block ID is required for flashing GPT")?;
        // Parses resize option.
        let resize = match next_arg(&mut args) {
            Some("resize") => true,
            Some(_) => return Err("Unknown argument".into()),
            _ => false,
        };
        Ok(Some((blk_id, resize)))
    }

    /// Helper for parsing partition argument.
    ///
    ///   <partition>/<blk id>/<offset>/<size>
    fn parse_partition_arg<'s>(
        &self,
        part: &'s str,
    ) -> CommandResult<(Option<&'s str>, Option<usize>, Option<u64>, Option<u64>)> {
        let mut args = part.split('/');
        // Parses partition name.
        let part = next_arg(&mut args);
        // Parses block device ID.
        let blk_id = self.check_next_arg_blk_id(&mut args)?;
        // Parses sub window offset.
        let window_offset = next_arg_u64(&mut args)?;
        // Parses sub window size.
        let window_size = next_arg_u64(&mut args)?;
        Ok((part, blk_id, window_offset, window_size))
    }

    /// Helper for resolving the block device ID and partition info for given optional partition
    /// name and block device ID.
    fn find_partition<'s>(
        &self,
        part: Option<&'s str>,
        blk_id: Option<usize>,
    ) -> Result<(usize, Partition), Error> {
        let devs = self.disks;
        // Checks uniqueness of the partition and resolves its block device ID.
        let find = |p: Option<&'s str>| match blk_id {
            None => Ok::<_, Error>(check_part_unique(devs, p.ok_or("Must provide a partition")?)?),
            Some(v) => Ok((v, devs[v].find_partition(p)?)),
        };
        Ok(match find(part) {
            // Some legacy Fuchsia devices in the field uses name "fuchsia-fvm" for the standard
            // "fvm" partition. However all of our infra uses the standard name "fvm" when flashing.
            // Here we do a one off mapping if the device falls into this case. Once we have a
            // solution for migrating those devices off the legacy name, we can remove this.
            //
            // If we run into more of such legacy aliases that we can't migrate, consider adding
            // interfaces in GblOps for this.
            #[cfg(feature = "fuchsia")]
            Err(Error::NotFound) if part == Some("fvm") => find(Some("fuchsia-fvm"))?,
            v => v?,
        })
    }

    /// Helper for finding partitions with special handling for slotted syntax. The following are
    /// checked in order:
    //
    //  1. If there is an exact match of the partition, return the match.
    //  2. If partition has format "<base>_ab", checks "<base>_a" and "<base>_b".
    //  3. Otherwise checks "<base>_<current slot>".
    fn resolve_slotted_partitions(
        &mut self,
        part: Option<&str>,
        blk_id: Option<usize>,
    ) -> Result<ArrayVec<(usize, Partition), MAX_IO_PARTS>, Error> {
        match self.find_partition(part, blk_id) {
            // If there is an exact match, returns as it is.
            Ok(v) => return Ok((&[v][..]).try_into().unwrap()),
            // Checks slotted extension. Currently only support A/B two-slot scheme.
            Err(Error::NotFound) => match part {
                // If the partition doesn't exist but ends with "_ab", expands to "_a" and "_b"
                // respectively.
                Some(p) if p.ends_with("_ab") => {
                    let mut res = ArrayVec::new();
                    let base = p.strip_suffix("_ab").unwrap();
                    for suffix in ["a", "b"] {
                        let part = RawName::new_formatted(format_args!("{base}_{suffix}"))?;
                        res.push(self.find_partition(Some(part.to_str()), blk_id)?);
                    }
                    Ok(res)
                }
                // Otherwise appends with current slot and checks again.
                Some(p) => {
                    let mut res = ArrayVec::new();
                    let slot = self.gbl_ops.get_current_slot()?.suffix.as_char();
                    let part = RawName::new_formatted(format_args!("{p}_{slot}"))?;
                    res.push(self.find_partition(Some(part.to_str()), blk_id)?);
                    Ok(res)
                }
                _ => Err(Error::NotFound),
            },
            Err(e) => return Err(e),
        }
    }

    /// Parses and resolves partition and wait until the corresponding IO can be obtained.
    pub(crate) async fn parse_and_get_partition_io<'s>(
        &mut self,
        part: &'s str,
    ) -> CommandResult<MultiPartitionIo<'a, B, MAX_IO_PARTS>> {
        let (part, blk_id, off, sz) = self.parse_partition_arg(part)?;
        let mut parts_info = ArrayVec::new();
        for (id, p) in self.resolve_slotted_partitions(part, blk_id)? {
            let (start, end) = p.sub(off, sz)?;
            parts_info.push((id, start, end));
        }
        loop {
            match crate::partition::create_multi_partition_io(self.disks, parts_info.clone()) {
                Err(Error::NotReady) => yield_now().await,
                v => return Ok(v?),
            }
        }
    }

    /// Takes the download data and resets download size.
    fn take_download(&mut self) -> Option<(ScopedBuffer<'b, P>, usize)> {
        Some((self.current_download_buffer.take()?, take(&mut self.current_download_size)))
    }

    /// Waits until a Disk device is ready and get the [PartitionIo] for `part`.
    pub async fn wait_partition_io(
        &self,
        blk: usize,
        part: Option<&str>,
    ) -> CommandResult<PartitionIo<'a, B>> {
        loop {
            match self.disks[blk].partition_io(part) {
                Err(Error::NotReady) => yield_now().await,
                v => return Ok(v?),
            }
        }
    }

    /// Helper for scheduling an async task.
    ///
    /// * If `Self::enable_async_task` is true, the method will add the task to the background task
    ///   list. Otherwise it simply runs the task.
    async fn schedule_task(
        &mut self,
        task: Task<'a, 'b, B, P>,
        responder: &mut impl InfoSender,
    ) -> CommandResult<()> {
        Ok(match self.enable_async_task {
            true => {
                let mut t = Some((self.task_mapper)(task));
                self.tasks.borrow_mut().add_with(|| t.take().unwrap());
                while t.is_some() {
                    yield_now().await;
                    self.tasks.borrow_mut().add_with(|| t.take().unwrap());
                }
                self.tasks.borrow_mut().poll_all();
                let info =
                    "An async task is launched. To sync manually, run \"oem gbl-sync-tasks\".";
                responder.send_info(info).await?
            }
            _ => task.run_checked().await?,
        })
    }

    /// Waits for all block devices to be ready.
    async fn sync_all_blocks(&self) -> CommandResult<()> {
        for (idx, _) in self.disks.iter().enumerate() {
            let _ = self.wait_partition_io(idx, None).await;
        }
        Ok(())
    }

    /// Implementation for "fastboot oem gbl-sync-tasks".
    async fn oem_sync_tasks(&self, mut _responder: impl InfoSender) -> CommandResult<()> {
        self.sync_all_blocks().await?;
        Ok(())
    }

    /// Sets the boot mode for the next reboot.
    fn set_boot_mode(&mut self, mode: AndroidBootMode) -> CommandResult<()> {
        match mode {
            #[cfg(feature = "fuchsia")]
            AndroidBootMode::BootloaderBootOnce if self.gbl_ops.expected_os_is_fuchsia()? => {
                set_one_shot_bootloader(&mut GblAbrOps(self.gbl_ops), true)?;
            }
            #[cfg(feature = "fuchsia")]
            AndroidBootMode::Recovery if self.gbl_ops.expected_os_is_fuchsia()? => {
                set_one_shot_recovery(&mut GblAbrOps(self.gbl_ops), true)?;
            }
            _ => {
                // Update the bootloader message (BCB) in the `misc` partition.
                let bcb =
                    read_bootloader_message_to(self.gbl_ops, self.data.boot_buffer.scratch())?;
                bcb.update_boot_command(mode);
                write_bootloader_message(self.gbl_ops, bcb)?;
            }
        }
        Ok(())
    }

    /// Syncs all storage devices and reboots.
    async fn sync_tasks_and_reboot(
        &mut self,
        mode: RebootMode,
        mut resp: impl InfoSender + OkaySender,
    ) -> CommandResult<!> {
        resp.send_info("Syncing storage...").await?;
        self.sync_all_blocks().await?;
        let msg = match mode {
            RebootMode::Normal => "Rebooting...",
            RebootMode::Bootloader => {
                self.set_boot_mode(AndroidBootMode::BootloaderBootOnce)?;
                "Rebooting to bootloader..."
            }
            RebootMode::Fastboot => {
                self.set_boot_mode(AndroidBootMode::Fastboot)?;
                "Rebooting to userspace fastboot..."
            }
            RebootMode::Recovery => {
                self.set_boot_mode(AndroidBootMode::Recovery)?;
                "Rebooting to recovery..."
            }
        };
        resp.send_info(msg).await?;
        resp.send_okay("").await?;
        self.gbl_ops.reboot()?
    }

    /// Appends a staged payload as bootloader file.
    #[cfg(feature = "fuchsia")]
    async fn add_staged_bootloader_file(&mut self, file_name: &str) -> CommandResult<()> {
        let buffer = self
            .gbl_ops
            .get_zbi_bootloader_files_buffer_aligned()
            .ok_or("No ZBI bootloader file buffer is provided")?;
        let data = self.current_download_buffer.as_mut().ok_or("No file staged")?;
        let data = &mut data[..self.current_download_size];
        let mut zbi = match ZbiContainer::parse(&mut buffer[..]) {
            Ok(v) => v,
            _ => ZbiContainer::new(&mut buffer[..])?,
        };
        let next_payload = zbi.get_next_payload()?;
        // Format: name length (1 byte) | name | file content.
        let (name_len, rest) = next_payload.split_at_mut_checked(1).ok_or("Buffer too small")?;
        let (name, rest) = rest.split_at_mut_checked(file_name.len()).ok_or("Buffer too small")?;
        let file_content = rest.get_mut(..data.len()).ok_or("Buffer too small")?;
        name_len[0] = file_name.len().try_into().map_err(|_| "File name length overflows 256")?;
        name.clone_from_slice(file_name.as_bytes());
        file_content.clone_from_slice(data);
        // Creates the entry;
        zbi.create_entry(
            ZbiType::BootloaderFile,
            0,
            Default::default(),
            1 + file_name.len() + data.len(),
        )?;
        Ok(())
    }

    /// Iterates all slots in the order of slot index.
    fn slots_iter(&mut self) -> Result<impl Iterator<Item = Result<Slot, Error>> + '_, Error> {
        Ok((0..self.gbl_ops.get_slot_count()?).map(|idx| self.gbl_ops.get_slot_info(idx)))
    }

    /// Sets active slot.
    async fn set_active_slot(&mut self, suffix: char) -> CommandResult<()> {
        self.sync_all_blocks().await?;

        #[cfg(feature = "fuchsia")]
        if self.gbl_ops.expected_os_is_fuchsia()? {
            // TODO(b/374776896): Prioritizes platform specific `set_active_slot`  if available.
            return Ok(mark_slot_active(
                &mut GblAbrOps(self.gbl_ops),
                match suffix {
                    'a' => SlotIndex::A,
                    'b' => SlotIndex::B,
                    _ => return Err("Invalid slot index for Fuchsia A/B/R".into()),
                },
            )?);
        }

        let idx = self
            .slots_iter()?
            .position(|slot| slot.is_ok() && slot.unwrap().suffix.as_char() == suffix)
            .ok_or("Invalid slot")?;
        Ok(self.gbl_ops.set_active_slot(idx as _)?)
    }

    /// Helper for "fastboot boot" in Android image.
    async fn boot_android(&mut self, img: &[u8], mut resp: impl InfoSender) -> CommandResult<()> {
        let slot = get_boot_slot(self.gbl_ops)?;
        let mut boot_part = [0u8; 16];
        let boot_part = snprintf!(boot_part, "boot_{}", slot.suffix.as_char());
        let mut ramboot_ops = RambootOps { ops: self.gbl_ops, ram_partitions: &[(boot_part, img)] };
        let boot_buffer = self.data.boot_buffer.as_borrowed();
        let (ramdisk, fdt, kernel, _) =
            android_load_verify_fixup(&mut ramboot_ops, slot, false, boot_buffer)?;
        self.result.loaded_image_info = LoadedImageInfo::Android {
            ramdisk: ramdisk.as_ptr_range(),
            fdt: fdt.as_ptr_range(),
            kernel: kernel.as_ptr_range(),
        };
        resp.send_formatted_info(|f| {
            write!(f, "Boot image as Android slot {}", slot.suffix.as_char()).unwrap()
        })
        .await?;
        Ok(())
    }

    /// Helper for "fastboot boot" Fuchsia image.
    #[cfg(feature = "fuchsia")]
    async fn boot_fuchsia(&mut self, img: &[u8], mut resp: impl InfoSender) -> CommandResult<()> {
        let load_buffer = self.data.boot_buffer.scratch();
        // Format is ZBI + Vbmeta.
        let (zbi, vbmeta) = zbi_split_unused_buffer_ref(get_kernel(img)?)?;
        let mut ramboot_ops = RambootOps {
            ops: self.gbl_ops,
            ram_partitions: &[
                ("zircon_a", zbi),
                ("vbmeta_a", vbmeta),
                ("zircon_b", zbi),
                ("vbmeta_b", vbmeta),
                ("zircon_r", zbi),
                ("vbmeta_r", vbmeta),
            ],
        };
        let LoadedVerifiedZircon { zbi_items, kernel, slot } =
            zircon_load_verify_abr_with_buffer(&mut ramboot_ops, load_buffer)?;
        self.result.loaded_image_info = LoadedImageInfo::Fuchsia {
            zbi_items: zbi_items.as_ptr_range(),
            kernel: kernel.as_ptr_range(),
            slot,
        };
        resp.send_formatted_info(|f| {
            write!(f, "Boot image as Fuchsia slot {}", char::from(slot)).unwrap()
        })
        .await?;
        Ok(())
    }

    /// Helper for dumping all partition info
    async fn oem_dump_partition_info(&mut self, mut resp: impl InfoSender) -> CommandResult<()> {
        let disks = self.disks;
        resp.send_formatted_info(|f| {
            write!(f, "<block ID>: <partition>, <range>, <size>").unwrap()
        })
        .await?;
        for (idx, blk) in disks.iter().enumerate() {
            for ptn_idx in 0..blk.num_partitions().unwrap_or(0) {
                let ptn = blk.get_partition_by_idx(ptn_idx)?;
                let sz: u64 = ptn.size()?;
                let part = ptn.name()?;
                let (start, end) = ptn.absolute_range()?;
                // Format  block ID, <partition name>, <range>, <size>
                resp.send_formatted_info(|f| {
                    write!(f, "{idx}: {part}, [{start:#x}, {end:#x}), {sz:#x}").unwrap()
                })
                .await?;
            }
        }
        Ok(())
    }

    /// Helper for checking and getting an `BootItemContainer` from general load buffer.
    fn boot_item_container(&mut self) -> CommandResult<&mut BootItemContainer<'b>> {
        let v = self.data.boot_buffer.boot_items();
        match v.check_valid() {
            Err(_) => v.init().map_err(|e| {
                CommandError::from(format_args!("Failed to initialize boot item container {e})"))
            })?,
            _ => {}
        }
        Ok(v)
    }

    /// Helper for checking whether device is unlocked.
    fn check_unlocked(&mut self) -> CommandResult<()> {
        match self.gbl_ops.fastboot_read_lock_state(LockType::Device) {
            Err(e) => Err(format_args!("Fail to get unlock status {e}").into()),
            Ok(LockState::Locked) => Err("Device is not unlocked".into()),
            _ => Ok(()),
        }
    }

    /// Gets or allocates download buffer.
    async fn get_download_buffer(&mut self) -> &mut [u8] {
        if self.current_download_buffer.is_none() {
            self.current_download_buffer = Some(self.buffer_pool.allocate_async().await);
        }
        self.current_download_buffer.as_mut().unwrap()
    }

    /// Helper for processing Self::load_result and emitting error messages.
    fn get_load_result(&self) -> CommandResult<LoadedImages<'b>> {
        match self.data.load_result {
            None => Err("Images not loaded. Run \"oem gbl-pause-fastboot-after-load\"".into()),
            Some(Err(e)) => Err(format_args!("Load didn't succeed: {e}").into()),
            Some(Ok(v)) => Ok(v),
        }
    }

    /// Upload OEM specific data.
    async fn upload_oem_data(
        &mut self,
        mut responder: impl UploadBuilder + InfoSender,
    ) -> CommandResult<()> {
        // Makes sure a download buffer can be allocated.
        if let Some(_) = self.take_download() {
            responder.send_info("A previous download is discarded.").await?;
        }
        self.get_download_buffer().await;
        let mut buffer = self.current_download_buffer.take().unwrap();
        let (_, total) = self.gbl_ops.fastboot_get_staged(&mut [][..])?;
        if total == 0 {
            return Err("No data staged.".into());
        } else if total >= 0x7fffffff {
            return Err("Cannot upload more than 0x7fffffff bytes of data".into());
        }

        responder
            .send_formatted_info(|v| write!(v, "Uploading {} bytes...", total).unwrap())
            .await?;
        // `total` already checked to be no more than 0x7fffffff.
        let mut uploader = responder.initiate_upload(total.try_into().unwrap()).await?;
        let mut left = total;
        let mut read_len: CommandResult<usize> = Ok(0);
        while left > 0 {
            read_len = read_len
                .and_then(|_| Ok(self.gbl_ops.fastboot_get_staged(&mut buffer)?))
                .and_then(|(read, remains)| match left >= remains && left - remains == read {
                    true => Ok(read),
                    _ => Err("Staged data size changed when uploading".into()),
                });
            // On success, upload the actual amount of read data. On any failure, continue to upload
            // arbitrary data until we pass the data phase, so that we can send the error message
            // and process future fastboot commands.
            let to_upload = read_len.as_ref().cloned().unwrap_or(min(left, buffer.len()));
            uploader.upload(&mut buffer[..to_upload]).await?;
            left -= to_upload
        }
        read_len?;
        Ok(())
    }
}

// See definition of [GblFastboot] for docs on lifetimes and generics parameters.
impl<'a: 'c, 'b: 'c, 'c, G, B, S, T, P, C, F> FastbootImplementation
    for GblFastboot<'a, 'b, 'c, '_, G, B, S, T, P, C, F>
where
    G: GblOps<'a>,
    B: BlockIo,
    S: DerefMut<Target = [u8]>,
    T: DerefMut<Target = [u8]>,
    P: BufferPool,
    C: PinFutContainerTyped<'c, F>,
    F: Future<Output = ()> + 'c,
{
    async fn get_var(
        &mut self,
        var: &CStr,
        args: impl Iterator<Item = &'_ CStr> + Clone,
        out: &mut [u8],
        _: impl InfoSender,
    ) -> CommandResult<usize> {
        Ok(self.get_var_internal(var, args, out).await?.len())
    }

    async fn get_var_all(&mut self, mut resp: impl VarInfoSender) -> CommandResult<()> {
        self.get_var_all_internal(&mut resp).await
    }

    async fn flash(&mut self, part: &str, mut responder: impl InfoSender) -> CommandResult<()> {
        let disks = self.disks;

        // Checks if we are flashing new GPT partition table
        if let Some((blk_idx, resize)) = self.parse_flash_gpt_args(part)? {
            self.wait_partition_io(blk_idx, None).await?;
            let (mut gpt, size) = self.take_download().ok_or("No GPT downloaded")?;
            responder.send_info("Updating GPT...").await?;
            return match disks[blk_idx].update_gpt(&mut gpt[..size], resize).await {
                Err(Error::NotReady) => panic!("Should not be busy"),
                Err(Error::Unsupported) => Err("Block device is not for GPT".into()),
                v => Ok(v?),
            };
        }

        let part_io = self.parse_and_get_partition_io(part).await?;
        let (data, sz) = self.take_download().ok_or("No download")?;
        let mut task = Task::new(match is_sparse_image(&data) {
            Ok(v) => TaskWorkload::FlashSparse(part_io.sub(0, v.data_size())?, data),
            _ => TaskWorkload::Flash(part_io.sub(0, sz.try_into().unwrap())?, data, sz),
        });
        task.set_context(|f| write!(f, "flash:{part}"));
        Ok(self.schedule_task(task, &mut responder).await?)
    }

    async fn erase(&mut self, part: &str, mut responder: impl InfoSender) -> CommandResult<()> {
        let disks = self.disks;

        // Checks if we are erasing GPT partition table.
        if let Some((blk_idx, _)) = self.parse_flash_gpt_args(part)? {
            self.wait_partition_io(blk_idx, None).await?;
            return match disks[blk_idx].erase_gpt().await {
                Err(Error::NotReady) => panic!("Should not be busy"),
                Err(Error::Unsupported) => Err("Block device is not for GPT".into()),
                v => Ok(v?),
            };
        }

        // If we are erasing full partition, checks vendor specific erase logic first.
        if part.find('/').is_none()
            && matches!(self.gbl_ops.fastboot_vendor_erase(part)?, FastbootEraseAction::Noop)
        {
            return Ok(());
        }

        let part_io = self.parse_and_get_partition_io(part).await?;
        self.get_download_buffer().await;
        let mut task = Task::new(TaskWorkload::Erase(part_io, self.take_download().unwrap().0));
        task.set_context(|f| write!(f, "erase:{part}"));
        Ok(self.schedule_task(task, &mut responder).await?)
    }

    async fn download(
        &mut self,
        responder: impl DownloadBuilder + InfoSender,
    ) -> CommandResult<()> {
        self.get_download_buffer().await;
        let buf = &mut self.current_download_buffer.as_mut().unwrap()[..];
        let total = responder.total();
        if total > buf.len() {
            return Err(
                format_args!("Buffer too small {:#x}. Needs {:#x}", buf.len(), total).into()
            );
        }

        let mut downloader = responder.initiate_download().await?;
        let channel = DataChannel::default();

        // CRC computation task.
        let crc = async {
            let Some(crc) = self.expected_download_crc.take() else {
                return Ok(());
            };
            let mut hasher = crc32fast::Hasher::new();
            while let Some(v) = channel.read().await? {
                // GBL's native TCP stack requires active CPU cycles to process incoming frames.
                // Hashing too much data causes ACK delay and increases retransmission rate which
                // significantly impacts download speed. Thus we yield after processing a moderate
                // amount. 4K is chosen empirically.
                //
                // Other media such as USB where hardware is capable of delivering final data is
                // not affected and as effective as without.
                for v in v.chunks(4 * 1024) {
                    hasher.update(v);
                    yield_now().await;
                }
            }
            let actual = hasher.finalize();
            if actual != crc {
                return Err(format_args!(
                    "CRC check failed. expect: {crc:#x}, actual: {actual:#x}"
                )
                .into());
            }
            Ok::<(), CommandError>(())
        };

        // Progress logging task.
        let log = async {
            if total < 128 * 1024 * 1024 {
                return Ok(());
            }
            let recv = || channel.remaining().map(|v| total - v);
            for i in 0..5 {
                while recv()? * 4 / total < i {
                    yield_now().await;
                }
                gbl_println!(self.gbl_ops, "\tDownloaded {}+%, {}/{total}", 25 * i, recv()?);
            }
            Ok::<(), Error>(())
        };

        let ((dl, crc), _) = join(join(download(&mut downloader, buf, &channel), crc), log).await;
        dl?;
        crc?;
        self.current_download_size = total;
        Ok(())
    }

    async fn upload(&mut self, responder: impl UploadBuilder + InfoSender) -> CommandResult<()> {
        let Some(data_type) = self.stage_data_type.take() else {
            return self.upload_oem_data(responder).await;
        };

        let data = match data_type {
            StageDataType::LoadedRamdisk => self.get_load_result()?.ramdisk,
            StageDataType::LoadedFdt => self.get_load_result()?.fdt,
            StageDataType::LoadedKernel => self.get_load_result()?.kernel,
        };
        let mut uploader = responder.initiate_upload(data.len().try_into().unwrap()).await?;
        Ok(uploader.upload(data).await?)
    }

    async fn fetch(
        &mut self,
        part: &str,
        offset: u64,
        size: u32,
        mut responder: impl UploadBuilder + InfoSender,
    ) -> CommandResult<()> {
        let mut part_io = self.parse_and_get_partition_io(part).await?;
        let buffer = self.get_download_buffer().await;
        // 4MB batches (chosen empirically) or as large as the download buffer permits.
        let batch_sz = min(4 * 1024 * 1024, buffer.len() / 2);
        let (mut read_buf, mut send_buf) = buffer.split_at_mut(batch_sz);
        let end = u64::try_from(SafeNum::from(offset) + size)?;
        let (mut read_off, mut upload_off) = (offset, offset);
        responder
            .send_formatted_info(|v| write!(v, "Uploading {} bytes...", size).unwrap())
            .await?;
        let mut uploader = responder.initiate_upload(size).await?;
        while upload_off < end {
            let to_send = min(usize::try_from(end - read_off)?, batch_sz);
            let read = part_io.read(read_off, &mut read_buf[..to_send]);
            let send = uploader.upload(&mut send_buf[..usize::try_from(read_off - upload_off)?]);
            let (read_res, send_res) = join(read, send).await;
            read_res.and_then(|_| send_res)?;
            upload_off = read_off;
            read_off += u64::try_from(to_send)?;
            core::mem::swap(&mut read_buf, &mut send_buf);
        }
        Ok(())
    }

    async fn reboot(
        &mut self,
        mode: RebootMode,
        resp: impl InfoSender + OkaySender,
    ) -> CommandResult<!> {
        self.sync_tasks_and_reboot(mode, resp).await
    }

    async fn r#continue(&mut self, mut resp: impl InfoSender) -> CommandResult<()> {
        resp.send_info("Syncing storage...").await?;
        Ok(self.sync_all_blocks().await?)
    }

    async fn set_active(&mut self, slot: &str, _: impl InfoSender) -> CommandResult<()> {
        if slot.len() > 1 {
            return Err("Slot suffix must be one character".into());
        }

        let slot_ch = slot.chars().next().ok_or("Invalid slot")?;
        self.set_active_slot(slot_ch).await?;
        self.result.last_set_active_slot = Some(slot_ch);
        Ok(())
    }

    async fn oem(
        &mut self,
        cmd_str: &str,
        mut responder: impl InfoSender + OkaySender + FailSender,
    ) -> CommandResult<()> {
        let mut args = cmd_str.split(' ');
        let cmd = args.next().ok_or("Missing command")?;
        self.stage_data_type = None;
        match cmd {
            "gbl-sync-tasks" => self.oem_sync_tasks(responder).await,
            "gbl-enable-async-task" => {
                self.enable_async_task = true;
                Ok(())
            }
            "gbl-disable-async-task" => {
                self.enable_async_task = false;
                Ok(())
            }
            "gbl-set-download-crc" => {
                // Longer term, if this feature is useful enough, it can be added to fastboot
                // upstream, i.e. download:<size>:<crc>
                let crc = next_arg_u64(&mut args)?.ok_or("Missing CRC32 value")?;
                self.expected_download_crc = Some(u32::try_from(crc)?);
                Ok(())
            }
            "gbl-unset-download-crc" => {
                self.expected_download_crc = None;
                Ok(())
            }
            "gbl-unset-default-block" => {
                self.default_block = None;
                Ok(())
            }
            "gbl-set-default-block" => {
                let id = next_arg_u64(&mut args)?.ok_or("Missing block device ID")?;
                let id = usize::try_from(id)?;
                self.disks.get(id).ok_or("Out of range")?;
                self.default_block = Some(id.try_into()?);
                responder
                    .send_formatted_info(|f| write!(f, "Default block device: {id:#x}").unwrap())
                    .await?;
                Ok(())
            }
            #[cfg(feature = "fuchsia")]
            "add-staged-bootloader-file" => {
                let file_name = next_arg(&mut args).ok_or("Missing file name")?;
                self.add_staged_bootloader_file(file_name).await?;
                Ok(())
            }
            "gbl-partition-info" => self.oem_dump_partition_info(responder).await,
            "gbl-add-cmdline" => {
                self.check_unlocked()?;
                let arg = next_arg(&mut args).ok_or("Missing cmdline arg")?;
                Ok(self.boot_item_container()?.append_item(BootItem::Cmdline, arg.as_bytes())?)
            }
            "gbl-add-bootconfig" => {
                self.check_unlocked()?;
                let arg = next_arg(&mut args).ok_or("Missing bootconfig arg")?;
                Ok(self.boot_item_container()?.append_item(BootItem::Bootconfig, arg.as_bytes())?)
            }
            "gbl-add-staged-data" => {
                self.check_unlocked()?;
                let arg = next_arg(&mut args).ok_or("Missing tag")?;
                let (data, sz) = self.take_download().ok_or("No download")?;
                Ok(self.boot_item_container()?.append_blob(arg, &data[..sz])?)
            }
            "gbl-stage" => {
                let arg = next_arg(&mut args).ok_or("Missing data type")?;
                let img_type = [
                    ("kernel", StageDataType::LoadedKernel),
                    ("ramdisk", StageDataType::LoadedRamdisk),
                    ("fdt", StageDataType::LoadedFdt),
                ];
                if let Some((_, v)) = img_type.iter().find(|(v, _)| *v == arg) {
                    self.get_load_result()?;
                    self.stage_data_type = Some(*v);
                    return Ok(());
                }
                return Err("Unknown data type".into());
            }
            "gbl-pause-fastboot-after-load" => {
                let v = next_arg_u64(&mut args)?.unwrap_or(1);
                self.result.pause_in_fastboot = v != 0;
                Ok(())
            }
            #[cfg(feature = "gbl_dev")]
            "stack-smash-demo" => {
                smash::stack_smash_demo(self.gbl_ops);
                Err("Stack smash demo failed to restart system".into())
            }
            _ => Err("Command not found".into()),
        }
    }

    async fn boot(&mut self, resp: impl InfoSender + OkaySender) -> CommandResult<()> {
        let (img, sz) = self.take_download().ok_or("No boot image staged")?;
        // Re-sync preloaded partitions. Device state or disk content might have changed due to
        // flashing etc.
        self.gbl_ops.sync_partition_buffer(true)?;
        #[cfg(feature = "fuchsia")]
        if is_fuchsia_fastboot_boot_image(&img[..sz]) {
            return self.boot_fuchsia(&img[..sz], resp).await;
        }

        self.boot_android(&img[..sz], resp).await
    }

    async fn flashing_write_lock_state(
        &mut self,
        lock_type: LockType,
        lock_state: LockState,
    ) -> CommandResult<()> {
        Ok(self.gbl_ops.avb_write_lock_state(lock_type, lock_state)?)
    }

    async fn flashing_get_unlock_ability(&mut self) -> CommandResult<Unlockability> {
        self.gbl_ops.fastboot_get_unlock_ability().map_err(|e| e.into())
    }

    async fn command_exec(
        &mut self,
        args: impl Iterator<Item = &'_ CStr> + Clone,
        responder: impl InfoSender + OkaySender + FailSender,
    ) -> CommandResult<CommandExecType> {
        let current_download_size =
            self.current_download_buffer.as_ref().map_or(0, |_| self.current_download_size);
        let res = self.gbl_ops.fastboot_command_exec(
            args.clone(),
            self.current_download_buffer.as_mut().map_or(&mut [][..], |v| &mut v[..]),
            current_download_size,
            responder,
        )?;

        Ok(res)
    }

    fn log_line(&mut self, message: impl Display) {
        gbl_println!(self.gbl_ops, "{}", message);
    }
}

#[cfg(feature = "gbl_dev")]
mod smash {
    use super::*;
    use libutils::get_sp;

    #[inline(never)]
    fn smash<'a>(ops: &mut impl GblOps<'a>, caller_sp: usize) {
        let sp = get_sp();
        let stack_size_bytes = caller_sp - sp;
        let terminus = (stack_size_bytes) / core::mem::size_of::<usize>();

        for i in 0..=terminus {
            // SAFETY: this is NOT SAFE.
            // It is a demo used to show stack smashing protection.
            // It DELIBERATELY violates stack integrity!
            //
            // This module requires the "gbl_dev" feature which precludes
            // use on production devices.
            unsafe {
                let ptr = (sp as *mut usize).add(i);
                gbl_println!(ops, "old val for stack @ sp + {} = 0x{:x}", i, *ptr);
                // Hack to keep RISC-V from crashing in the wrong way
                // for the wrong reason :P
                if i > 4 {
                    *ptr += 1;
                }
            }
        }
        gbl_println!(ops, "Finished smashing stack");
    }

    #[inline(never)]
    pub(super) fn stack_smash_demo<'a>(ops: &mut impl GblOps<'a>) {
        gbl_println!(ops, "Stack smashing demo");
        let base_stack = get_sp();
        smash(ops, base_stack);
    }
}

/// `GblGenericTransport` defines transport interfaces for running GBL fastboot over
/// EfiFastbootTransport.
pub trait GblGenericTransport: Transport {
    /// Checks whether there is a new packet.
    fn has_packet(&mut self) -> bool;
}

/// `GblTcpStream` defines transport interfaces for running GBL fastboot over TCP.
pub trait GblTcpStream: TcpStream {
    /// Accepts a new TCP connection.
    ///
    /// If a connection is in progress, it should be aborted first.
    ///
    /// Returns true if a new connection is established, false otherwise.
    fn accept_new(&mut self) -> bool;
}

#[derive(Default)]
enum Request<'a> {
    #[default]
    Pending,
    Ready(&'a mut [u8]),
}

/// Contains state data of a `DataChannel`
#[derive(Default)]
struct DataChannelInternal<'a> {
    // Represents a read request made by `DataChannel::read`.
    //
    // `None`: No data read request.
    // `Some(Request::Pending)`: Request is pending.
    // `Some(Request::Ready(_))`: Request data is ready.
    request: Option<Request<'a>>,
    // Stores the result when download is complete.
    //
    // Completed: Some(Ok(<unread data>))
    // Failed: Some(Err(_))
    result: Option<Result<&'a mut [u8], Error>>,
    // Remaining data from the download
    remaining: usize,
}

/// `DataChannel` contains shared state and buffer for async code to read and download data in
/// parallel.
#[derive(Default)]
struct DataChannel<'a>(RefCell<DataChannelInternal<'a>>);

impl<'a> DataChannel<'a> {
    // Waits and reads data from the channel.
    async fn read(&self) -> Result<Option<&'a mut [u8]>, Error> {
        loop {
            if let Some(Request::Ready(v)) = self.0.borrow_mut().request.take() {
                return Ok(Some(v));
            }
            match &mut self.0.borrow_mut().result {
                Some(Ok(v)) => return Ok((!v.is_empty()).then_some(take(v))),
                Some(Err(e)) => return Err(*e),
                _ => {}
            }
            self.0.borrow_mut().request = Some(Request::Pending);
            yield_now().await;
        }
    }

    // Returns the remaining size of data from the download.
    fn remaining(&self) -> Result<usize, Error> {
        if let Some(Err(e)) = self.0.borrow().result.as_ref() {
            return Err(*e);
        }
        Ok(self.0.borrow().remaining)
    }
}

/// Download data to the given buffer until either the buffer is filled or download is complete.
///
/// # Args:
///
/// * `downloader`: The `Downloader` for receiving download data.
/// * `buffer`: The target buffer to receive the data.
/// * `channel`: A `DataChannel` for other async code to read downloaded data in parallel.
async fn download<'a>(
    downloader: &mut impl Downloader,
    buffer: &'a mut [u8],
    channel: &DataChannel<'a>,
) -> Result<(), Error> {
    let res = async {
        let sz = min(downloader.remaining(), buffer.len());
        let mut to_download = &mut buffer[..sz];
        let mut downloaded;
        let mut curr = 0;
        channel.0.borrow_mut().remaining = downloader.remaining();
        while curr < to_download.len() {
            curr += downloader.download(&mut to_download[curr..]).await?;
            channel.0.borrow_mut().remaining = downloader.remaining();
            if matches!(channel.0.borrow_mut().request, Some(Request::Pending)) {
                (downloaded, to_download) = to_download.split_at_mut(take(&mut curr));
                channel.0.borrow_mut().request = Some(Request::Ready(downloaded));
            }
        }
        Ok(to_download)
    }
    .await;
    channel.0.borrow_mut().result.insert(res).as_ref().map_err(|e| *e)?;
    Ok(())
}

/// Runs GBL fastboot on the given USB/TCP channels.
///
/// # Args:
///
/// * `gbl_ops`: An instance of [GblOps].
/// * `buffer_pool`: An implementation of [BufferPool].
/// * `tasks`: An implementation of [PinFutContainer]
/// * `transports`: Implementations of [GblGenericTransport].
/// * `tcp`: An optional implementation of [GblTcpStream].
///
/// # Lifetimes
/// * `'a`: Lifetime of [GblOps].
/// * `'b`: Lifetime of `download_buffers`.
/// * `'c`: Lifetime of `tasks`.
pub(crate) async fn run_gbl_fastboot<'a: 'c, 'b: 'c, 'c>(
    gbl_ops: &mut impl GblOps<'a>,
    buffer_pool: &'b Shared<impl BufferPool>,
    tasks: impl PinFutContainer<'c> + 'c,
    transports: &mut [impl GblGenericTransport],
    tcp: Option<impl GblTcpStream>,
    data: GblFbData<'b>,
) -> GblFastbootResult {
    let tasks = tasks.into();
    let disks = gbl_ops.disks();
    let mut fb = GblFastboot::new(gbl_ops, disks, Task::run, &tasks, buffer_pool, data);
    fb.run(transports, tcp).await;
    fb.result
}

/// Runs GBL fastboot on the given transports' channels with N stack allocated worker tasks.
///
/// The choice of N depends on the level of parallelism the platform can support. For platform with
/// `n` storage devices that can independently perform non-blocking IO, it will required `N = n`
/// and a `buffer_pool` that can allocate at least n+1 buffers at the same time in order to achieve
/// parallel flashing to all storages plus a parallel downloading. However, it is common for
/// partitions that need to be flashed to be on the same block deviece so flashing of them becomes
/// sequential, in which case N can be smaller. Caller should take into consideration usage pattern
/// for determining N.
///
/// # Args:
///
/// * `gbl_ops`: An instance of [GblOps].
/// * `buffer_pool`: An implementation of [BufferPool].
/// * `transports`: Implementations of [GblGenericTransport].
/// * `tcp`: An optional implementation of [GblTcpStream].
pub(crate) async fn run_gbl_fastboot_stack<'a, const N: usize>(
    gbl_ops: &mut impl GblOps<'a>,
    buffer_pool: impl BufferPool,
    transports: &mut [impl GblGenericTransport],
    tcp: Option<impl GblTcpStream>,
    data: GblFbData<'_>,
) -> GblFastbootResult {
    let buffer_pool = buffer_pool.into();
    // Creates N worker tasks.
    let mut tasks: [_; N] = from_fn(|_| Task::default().run());
    // It is possible to avoid the use of the unsafe `Pin::new_unchecked` by delaring the array and
    // manually pinning each element i.e.
    //
    // ```
    // let mut tasks = [
    //     core::pin::pin!(Task::None.run()),
    //     core::pin::pin!(Task::None.run()),
    //     core::pin::pin!(Task::None.run()),
    // ];
    // ```
    //
    // Parameterization of `N` will be an issue, but might be solvable with procedural macro.
    // SAFETY: `tasks` is immediately shadowed and thus guaranteed not moved for the rest of its
    // lifetime.
    let mut tasks: [_; N] = tasks.each_mut().map(|v| unsafe { Pin::new_unchecked(v) });
    let tasks = PinFutSlice::new(&mut tasks[..]).into();
    let disks = gbl_ops.disks();
    let mut fb = GblFastboot::new(gbl_ops, disks, Task::run, &tasks, &buffer_pool, data);
    fb.run(transports, tcp).await;
    fb.result
}

/// Pre-generates a Fuchsia Fastboot MDNS service broadcast packet.
///
/// Fuchsia ffx development flow can detect fastboot devices that broadcast a "_fastboot·_tcp"
/// MDNS service. This API generates the broadcast MDNS packet for Ipv6. Caller is reponsible for
/// sending this packet via UDP at the following address and port (defined by MDNS):
///
/// * ipv6: ff02::fb
/// * port: 5353
///
/// # Args
///
/// * `node_name`: The Fuchsia node name for the service. Must be a 22 character ASCII string in the
///   format "fuchsia-xxxx-xxxx-xxxx".
/// * `ipv6_addr`: The Ipv6 address bytes.
///
/// The packet generated by the API contains the given IPv6 address and a fuchsia node name derived
/// from the given ethernet mac address `eth_mac`.
pub fn fuchsia_fastboot_mdns_packet(node_name: &str, ipv6_addr: &[u8]) -> Result<[u8; 140], Error> {
    // Pre-generated Fuchsia fastboot MDNS service packet template.
    // It contains the node name and ipv6 address. We simply replace with the device's node name and
    // ipv6 address.
    let mut packet: [u8; 140] = [
        0x00, 0x00, 0x84, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x09, 0x5f, 0x66,
        0x61, 0x73, 0x74, 0x62, 0x6f, 0x6f, 0x74, 0x04, 0x5f, 0x74, 0x63, 0x70, 0x05, 0x6c, 0x6f,
        0x63, 0x61, 0x6c, 0x00, 0x00, 0x0c, 0x80, 0x01, 0x00, 0x00, 0x00, 0x78, 0x00, 0x19, 0x16,
        0x66, 0x75, 0x63, 0x68, 0x73, 0x69, 0x61, 0x2d, 0x34, 0x38, 0x32, 0x31, 0x2d, 0x30, 0x62,
        0x33, 0x31, 0x2d, 0x65, 0x61, 0x66, 0x38, 0xc0, 0x0c, 0xc0, 0x2c, 0x00, 0x21, 0x80, 0x01,
        0x00, 0x00, 0x00, 0x78, 0x00, 0x1f, 0x00, 0x00, 0x00, 0x00, 0x15, 0xb2, 0x16, 0x66, 0x75,
        0x63, 0x68, 0x73, 0x69, 0x61, 0x2d, 0x34, 0x38, 0x32, 0x31, 0x2d, 0x30, 0x62, 0x33, 0x31,
        0x2d, 0x65, 0x61, 0x66, 0x38, 0xc0, 0x1b, 0xc0, 0x57, 0x00, 0x1c, 0x80, 0x01, 0x00, 0x00,
        0x00, 0x78, 0x00, 0x10, 0xfe, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4a, 0x21, 0x0b,
        0xff, 0xfe, 0x31, 0xea, 0xf8,
    ];
    // Offsets to the fuchsia node name field.
    const NODE_NAME_OFFSETS: &[usize; 2] = &[45, 88];
    // Offset to the IPv6 address field.
    const IP6_ADDR_OFFSET: usize = 124;

    if node_name.as_bytes().len() != 22 {
        return Err(Error::InvalidInput);
    }

    for off in NODE_NAME_OFFSETS {
        packet[*off..][..node_name.len()].clone_from_slice(node_name.as_bytes());
    }
    packet[IP6_ADDR_OFFSET..][..ipv6_addr.len()].clone_from_slice(ipv6_addr);
    Ok(packet)
}

/// Checks if a fastboot boot image is a fuchsia image.
#[cfg(feature = "fuchsia")]
fn is_fuchsia_fastboot_boot_image(img: &[u8]) -> bool {
    get_kernel(img).and_then(|v| ZbiContainer::parse(v).map_err(|_| Error::Other(None))).is_ok()
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use crate::{
        android_boot::tests::{
            checks_loaded_v2_slot_a_normal_mode, checks_loaded_v2_slot_b_normal_mode,
            default_test_gbl_ops, read_test_data,
        },
        constants::{KiB, MiB, KERNEL_ALIGNMENT},
        gbl_avb::AvbDeviceStatus,
        misc::test::read_bootloader_message,
        ops::{
            test::{
                into_refmut_bytes, slot, slot_successful, slot_unbootable, FakeGblOps,
                FakeGblOpsStorage, SenderMessage,
            },
            Partition, PartitionBuffer,
        },
        Os,
    };
    #[cfg(feature = "fuchsia")]
    use abr::{
        get_and_clear_one_shot_bootloader, get_boot_slot, mark_slot_unbootable, ABR_DATA_SIZE,
    };
    use core::{
        mem::size_of,
        pin::{pin, Pin},
    };
    use fastboot::{test_utils::TestUploadBuilder, CommandExecType, MAX_RESPONSE_SIZE};
    use gbl_async::{block_on, poll, poll_n_times};
    use gbl_storage::GPT_GUID_LEN;
    use libbuild_number::BUILD_NUMBER;
    use liberror::Error;
    use libtestutils::AlignedBuffer;
    use spin::{Mutex, MutexGuard};
    use std::{
        cell::RefCell,
        collections::{HashMap, VecDeque},
        ffi::CString,
        io::Read,
    };
    #[cfg(feature = "fuchsia")]
    use zerocopy::IntoBytes;

    /// A test implementation of [InfoSender] and [OkaySender].
    #[derive(Default)]
    struct TestResponder {
        okay_sent: Mutex<bool>,
        info_messages: Mutex<Vec<String>>,
    }

    impl InfoSender for &TestResponder {
        async fn send_formatted_info<F: FnOnce(&mut dyn Write)>(
            &mut self,
            cb: F,
        ) -> Result<(), Error> {
            let mut msg: String = "".into();
            cb(&mut msg);
            self.info_messages.try_lock().unwrap().push(msg);
            Ok(())
        }
    }

    impl OkaySender for &TestResponder {
        async fn send_formatted_okay<F: FnOnce(&mut dyn Write)>(self, _: F) -> Result<(), Error> {
            *self.okay_sent.try_lock().unwrap() = true;
            Ok(())
        }
    }

    impl FailSender for &TestResponder {
        async fn send_formatted_fail<F: FnOnce(&mut dyn Write)>(self, _: F) -> Result<(), Error> {
            Ok(())
        }
    }

    /// Helper to test fastboot variable value.
    fn check_var(gbl_fb: &mut impl FastbootImplementation, var: &str, args: &str, expected: &str) {
        let resp: TestResponder = Default::default();
        let args_c = args.split(':').map(|v| CString::new(v).unwrap()).collect::<Vec<_>>();
        let args_c = args_c.iter().map(|v| v.as_c_str());
        let var_c = CString::new(var).unwrap();
        let mut out = vec![0u8; MAX_RESPONSE_SIZE];
        let val =
            block_on(gbl_fb.get_var_as_str(var_c.as_c_str(), args_c, &resp, &mut out[..])).unwrap();
        assert_eq!(val, expected, "var {}:{} = {} != {}", var, args, val, expected,);
    }

    /// A helper to set the download content.
    fn set_download(gbl_fb: &mut impl FastbootImplementation, data: &[u8]) {
        block_on(gbl_fb.set_download(data)).unwrap()
    }

    impl<'a> PinFutContainer<'a> for Vec<Pin<Box<dyn Future<Output = ()> + 'a>>> {
        fn add_with<F: Future<Output = ()> + 'a>(&mut self, f: impl FnOnce() -> F) {
            self.push(Box::pin(f()));
        }

        fn for_each_remove_if(
            &mut self,
            mut cb: impl FnMut(&mut Pin<&mut (dyn Future<Output = ()> + 'a)>) -> bool,
        ) {
            for idx in (0..self.len()).rev() {
                cb(&mut self[idx].as_mut()).then(|| self.swap_remove(idx));
            }
        }
    }

    #[test]
    fn test_get_var_gbl() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 1]);
        let storage = FakeGblOpsStorage::default();
        let mut gbl_ops = FakeGblOps::new(&storage);
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);
        check_var(
            &mut gbl_fb,
            FakeGblOps::GBL_TEST_VAR,
            "arg",
            format!("{}:Some(\"arg\")", FakeGblOps::GBL_TEST_VAR_VAL).as_str(),
        );
    }

    #[test]
    fn test_get_var_slot_info() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 1]);
        let storage = FakeGblOpsStorage::default();
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.slot_count = Some(Ok(3));
        gbl_ops.slot_infos =
            vec![Ok(slot('a')), Ok(slot_successful('b')), Ok(slot_unbootable('c'))];
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);

        check_var(&mut gbl_fb, "slot-count", "", "3");
        check_var(&mut gbl_fb, "current-slot", "", "a");
        check_var(&mut gbl_fb, "slot-successful", "a", "no");
        check_var(&mut gbl_fb, "slot-unbootable", "a", "no");
        check_var(&mut gbl_fb, "slot-retry-count", "a", "7");
        check_var(&mut gbl_fb, "slot-successful", "b", "yes");
        check_var(&mut gbl_fb, "slot-unbootable", "b", "no");
        check_var(&mut gbl_fb, "slot-retry-count", "b", "0");
        check_var(&mut gbl_fb, "slot-successful", "c", "no");
        check_var(&mut gbl_fb, "slot-unbootable", "c", "yes");
        check_var(&mut gbl_fb, "slot-retry-count", "c", "0");
    }

    #[test]
    fn test_get_var_partition_has_slot() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 1]);
        let mut load_buffer = AlignedBuffer::new(MiB!(8), KERNEL_ALIGNMENT);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_2.bin"));
        storage.add_raw_device(c"raw_0", [0xaau8; KiB!(4)]);
        storage.add_raw_device(c"raw_1", [0x55u8; KiB!(8)]);
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.slot_count = Some(Ok(3));
        gbl_ops.slot_infos =
            vec![Ok(slot('a')), Ok(slot_successful('b')), Ok(slot_unbootable('c'))];
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let mut gbl_fb = GblFastboot::new(
            &mut gbl_ops,
            parts,
            Task::run,
            &tasks,
            &dl_buffers,
            GblFbData { boot_buffer: load_buffer.as_mut().into(), ..Default::default() },
        );

        check_var(&mut gbl_fb, "has-slot", "boot", "yes");
        check_var(&mut gbl_fb, "has-slot", "vendor_boot", "yes");
        check_var(&mut gbl_fb, "has-slot", "raw_0", "no");
        check_var(&mut gbl_fb, "has-slot", "raw_1", "no");
        let resp: TestResponder = Default::default();
        let mut out = vec![0u8; MAX_RESPONSE_SIZE];
        assert!(block_on(gbl_fb.get_var_as_str(c"has-slot", [].into_iter(), &resp, &mut out[..]))
            .is_err());
        assert!(block_on(gbl_fb.get_var_as_str(
            c"has-slot",
            [c"boot_a"].into_iter(),
            &resp,
            &mut out[..],
        ))
        .is_err());
        assert!(block_on(gbl_fb.get_var_as_str(
            c"has-slot",
            [c"boot_b"].into_iter(),
            &resp,
            &mut out[..],
        ))
        .is_err());
        assert!(block_on(gbl_fb.get_var_as_str(
            c"has-slot",
            [c"raw"].into_iter(),
            &resp,
            &mut out[..],
        ))
        .is_err());
    }

    #[test]
    fn test_get_var_partition_info() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 1]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_2.bin"));
        storage.add_raw_device(c"raw_0", [0xaau8; KiB!(4)]);
        storage.add_raw_device(c"raw_1", [0x55u8; KiB!(8)]);
        let mut gbl_ops = FakeGblOps::new(&storage);
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);

        // Check different semantics
        check_var(&mut gbl_fb, "partition-size", "boot_a", "0x2000");
        check_var(&mut gbl_fb, "partition-size", "boot_a/", "0x2000");
        check_var(&mut gbl_fb, "partition-size", "boot_a//", "0x2000");
        check_var(&mut gbl_fb, "partition-size", "boot_a///", "0x2000");
        check_var(&mut gbl_fb, "partition-size", "boot_a/0", "0x2000");
        check_var(&mut gbl_fb, "partition-size", "boot_a/0/", "0x2000");
        check_var(&mut gbl_fb, "partition-size", "boot_a//0", "0x2000");
        check_var(&mut gbl_fb, "partition-size", "boot_a/0/0", "0x2000");
        check_var(&mut gbl_fb, "partition-size", "boot_a//0x1000", "0x1000");

        check_var(&mut gbl_fb, "partition-size", "boot_b/0", "0x3000");
        check_var(&mut gbl_fb, "partition-size", "vendor_boot_a/1", "0x1000");
        check_var(&mut gbl_fb, "partition-size", "vendor_boot_b/1", "0x1800");
        check_var(&mut gbl_fb, "partition-size", "boot_a//0x1000", "0x1000");
        check_var(&mut gbl_fb, "partition-size", "raw_0", "0x1000");
        check_var(&mut gbl_fb, "partition-size", "raw_1", "0x2000");

        check_var(&mut gbl_fb, "partition-guid", "boot_a", "42aaac2e-37e3-43ba-9930-42dfa96e6334");
        check_var(&mut gbl_fb, "partition-guid", "boot_b", "bdadfeca-879c-43e9-8f0d-8ef7da29b5e7");

        let resp: TestResponder = Default::default();
        let mut out = vec![0u8; MAX_RESPONSE_SIZE];
        // GUID cannot be reported for raw partition.
        assert!(block_on(gbl_fb.get_var_as_str(
            c"partition-guid",
            [c"raw_1"].into_iter(),
            &resp,
            &mut out[..],
        ))
        .is_err());
        assert!(block_on(gbl_fb.get_var_as_str(
            c"partition-guid",
            [c"non-existent"].into_iter(),
            &resp,
            &mut out[..],
        ))
        .is_err());
        assert!(block_on(gbl_fb.get_var_as_str(
            c"partition",
            [c"non-existent"].into_iter(),
            &resp,
            &mut out[..],
        ))
        .is_err());
    }

    /// `TestVarSender` implements `TestVarSender`. It stores outputs in a vector of string.
    struct TestVarSender(Vec<String>);

    impl VarInfoSender for &mut TestVarSender {
        async fn send_var_info(
            &mut self,
            name: &str,
            args: impl IntoIterator<Item = &'_ str>,
            val: &str,
        ) -> Result<(), Error> {
            let args = [vec![name], args.into_iter().collect::<Vec<_>>()].concat();
            self.0.push(format!("{}: {}", args.join(":"), val));
            Ok(())
        }
    }

    /// Returns the expected value of the `version-bootloader` variable.
    fn expected_version_bootloader() -> String {
        format!("gbl.{BUILD_NUMBER}")
    }

    #[test]
    fn test_get_var_all() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 1]);
        let mut load_buffer = AlignedBuffer::new(MiB!(8), KERNEL_ALIGNMENT);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_2.bin"));
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_2.bin"));
        storage.add_raw_device(c"raw_0", [0xaau8; KiB!(4)]);
        storage.add_raw_device(c"raw_1", [0x55u8; KiB!(8)]);
        storage.add_raw_device(c"raw_1", [0x55u8; KiB!(8)]);
        let mut gbl_ops = FakeGblOps::new(&storage);
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let mut gbl_fb = GblFastboot::new(
            &mut gbl_ops,
            parts,
            Task::run,
            &tasks,
            &dl_buffers,
            GblFbData { boot_buffer: load_buffer.as_mut().into(), ..Default::default() },
        );

        let mut logger = TestVarSender(vec![]);
        block_on(gbl_fb.get_var_all(&mut logger)).unwrap();
        assert_eq!(
            logger.0,
            [
                "max-download-size: 0x20000",
                format!("version-bootloader: {}", expected_version_bootloader()).as_str(),
                "slot-count: 2",
                "current-slot: a",
                "slot-successful:a: no",
                "slot-unbootable:a: no",
                "slot-retry-count:a: 7",
                "slot-successful:b: no",
                "slot-unbootable:b: no",
                "slot-retry-count:b: 7",
                "has-slot:boot: yes",
                "has-slot:vendor_boot: yes",
                "has-slot:raw_0: no",
                "has-slot:raw_1: no",
                "max-fetch-size: 0x7fffffff",
                "block-device:0:total-blocks: 0x80",
                "block-device:0:block-size: 0x200",
                "block-device:0:status: idle",
                "block-device:1:total-blocks: 0x100",
                "block-device:1:block-size: 0x200",
                "block-device:1:status: idle",
                "block-device:2:total-blocks: 0x100",
                "block-device:2:block-size: 0x200",
                "block-device:2:status: idle",
                "block-device:3:total-blocks: 0x1000",
                "block-device:3:block-size: 0x1",
                "block-device:3:status: idle",
                "block-device:4:total-blocks: 0x2000",
                "block-device:4:block-size: 0x1",
                "block-device:4:status: idle",
                "block-device:5:total-blocks: 0x2000",
                "block-device:5:block-size: 0x1",
                "block-device:5:status: idle",
                "gbl-default-block: None",
                "partition-size:boot_a: 0x2000",
                "partition-type:boot_a: raw",
                "partition-guid:boot_a: 42aaac2e-37e3-43ba-9930-42dfa96e6334",
                "partition-size:boot_b: 0x3000",
                "partition-type:boot_b: raw",
                "partition-guid:boot_b: bdadfeca-879c-43e9-8f0d-8ef7da29b5e7",
                "partition-size:vendor_boot_a/1: 0x1000",
                "partition-type:vendor_boot_a/1: raw",
                "partition-guid:vendor_boot_a/1: 42aaac2e-37e3-43ba-9930-42dfa96e6334",
                "partition-size:vendor_boot_b/1: 0x1800",
                "partition-type:vendor_boot_b/1: raw",
                "partition-guid:vendor_boot_b/1: bdadfeca-879c-43e9-8f0d-8ef7da29b5e7",
                "partition-size:vendor_boot_a/2: 0x1000",
                "partition-type:vendor_boot_a/2: raw",
                "partition-guid:vendor_boot_a/2: 42aaac2e-37e3-43ba-9930-42dfa96e6334",
                "partition-size:vendor_boot_b/2: 0x1800",
                "partition-type:vendor_boot_b/2: raw",
                "partition-guid:vendor_boot_b/2: bdadfeca-879c-43e9-8f0d-8ef7da29b5e7",
                "partition-size:raw_0: 0x1000",
                "partition-type:raw_0: raw",
                "partition-size:raw_1/4: 0x2000",
                "partition-type:raw_1/4: raw",
                "partition-size:raw_1/5: 0x2000",
                "partition-type:raw_1/5: raw",
                "unlocked: no",
                "unlocked-critical: no",
                format!("{}:1: {}:1", FakeGblOps::GBL_TEST_VAR, FakeGblOps::GBL_TEST_VAR_VAL)
                    .as_str(),
                format!("{}:2: {}:2", FakeGblOps::GBL_TEST_VAR, FakeGblOps::GBL_TEST_VAR_VAL)
                    .as_str(),
                format!(
                    "{}: {}",
                    FakeGblOps::GBL_TEST_VAR_UNSPLIT,
                    FakeGblOps::GBL_TEST_VAR_UNSPLIT_VAL
                )
                .as_str(),
            ]
        );
    }

    #[cfg(feature = "gbl_dev")]
    #[test]
    fn test_get_var_all_skips_slot_count_if_unsupported() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 1]);
        let storage = FakeGblOpsStorage::default();
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.slot_count = Some(Err(Error::Unsupported));
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);

        let mut logger = TestVarSender(vec![]);
        block_on(gbl_fb.get_var_all(&mut logger)).unwrap();
        assert_eq!(
            logger.0,
            [
                "max-download-size: 0x20000",
                format!("version-bootloader: {}", expected_version_bootloader()).as_str(),
                "max-fetch-size: 0x7fffffff",
                "gbl-default-block: None",
                "unlocked: no",
                "unlocked-critical: no",
                format!("{}:1: {}:1", FakeGblOps::GBL_TEST_VAR, FakeGblOps::GBL_TEST_VAR_VAL)
                    .as_str(),
                format!("{}:2: {}:2", FakeGblOps::GBL_TEST_VAR, FakeGblOps::GBL_TEST_VAR_VAL)
                    .as_str(),
                format!(
                    "{}: {}",
                    FakeGblOps::GBL_TEST_VAR_UNSPLIT,
                    FakeGblOps::GBL_TEST_VAR_UNSPLIT_VAL
                )
                .as_str(),
            ]
        );
    }

    #[test]
    fn test_get_var_all_unsupported_slot_info() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 1]);
        let storage = FakeGblOpsStorage::default();
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.slot_count = Some(Ok(2));
        gbl_ops.slot_infos = vec![Err(Error::Unsupported), Err(Error::Unsupported)];
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);

        let mut logger = TestVarSender(vec![]);
        block_on(gbl_fb.get_var_all(&mut logger)).unwrap();
        assert_eq!(
            logger.0,
            [
                "max-download-size: 0x20000",
                format!("version-bootloader: {}", expected_version_bootloader()).as_str(),
                "slot-count: 2",
                "max-fetch-size: 0x7fffffff",
                "gbl-default-block: None",
                "unlocked: no",
                "unlocked-critical: no",
                format!("{}:1: {}:1", FakeGblOps::GBL_TEST_VAR, FakeGblOps::GBL_TEST_VAR_VAL)
                    .as_str(),
                format!("{}:2: {}:2", FakeGblOps::GBL_TEST_VAR, FakeGblOps::GBL_TEST_VAR_VAL)
                    .as_str(),
                format!(
                    "{}: {}",
                    FakeGblOps::GBL_TEST_VAR_UNSPLIT,
                    FakeGblOps::GBL_TEST_VAR_UNSPLIT_VAL
                )
                .as_str(),
            ]
        );
    }

    /// A helper for fetching partition from a `GblFastboot`
    fn fetch<EOff: core::fmt::Debug, ESz: core::fmt::Debug>(
        fb: &mut impl FastbootImplementation,
        part: String,
        off: impl TryInto<u64, Error = EOff>,
        size: impl TryInto<u32, Error = ESz>,
    ) -> CommandResult<Vec<u8>> {
        let off = off.try_into().unwrap();
        let size = size.try_into().unwrap();
        let mut upload_out = vec![0u8; usize::try_from(size).unwrap()];
        let test_uploader = TestUploadBuilder(&mut upload_out[..]);
        block_on(fb.fetch(part.as_str(), off, size, test_uploader))?;
        Ok(upload_out)
    }

    #[test]
    fn test_fetch_invalid_partition_arg() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 1]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_2.bin"));
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_2.bin"));
        let mut gbl_ops = FakeGblOps::new(&storage);
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);

        // Missing mandatory block device ID for raw block partition.
        assert!(fetch(&mut gbl_fb, "//0/0".into(), 0, 0).is_err());

        // GPT partition does not exist.
        assert!(fetch(&mut gbl_fb, "non///".into(), 0, 0).is_err());

        // GPT Partition is not unique.
        assert!(fetch(&mut gbl_fb, "vendor_boot_a///".into(), 0, 0).is_err());

        // Offset overflows.
        assert!(fetch(&mut gbl_fb, "boot_a//0x2001/".into(), 0, 1).is_err());
        assert!(fetch(&mut gbl_fb, "boot_a".into(), 0x2000, 1).is_err());

        // Size overflows.
        assert!(fetch(&mut gbl_fb, "boot_a///0x2001".into(), 0, 0).is_err());
        assert!(fetch(&mut gbl_fb, "boot_a".into(), 0, 0x2001).is_err());
    }

    /// A helper for testing raw block upload. It verifies that data read from block device
    /// `blk_id` in range [`off`, `off`+`size`) is the same as `disk[off..][..size]`
    fn check_blk_upload(
        fb: &mut impl FastbootImplementation,
        blk_id: u64,
        off: u64,
        size: u64,
        disk: &[u8],
    ) {
        let expected = disk[off.try_into().unwrap()..][..size.try_into().unwrap()].to_vec();
        // offset/size as part of the partition string.
        let part = format!("/{:#x}/{:#x}/{:#x}", blk_id, off, size);
        assert_eq!(fetch(fb, part, 0, size).unwrap(), expected);
        // offset/size as separate fetch arguments.
        let part = format!("/{:#x}", blk_id);
        assert_eq!(fetch(fb, part, off, size).unwrap(), expected);
    }

    #[test]
    fn test_fetch_raw_block() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 1]);
        let mut storage = FakeGblOpsStorage::default();
        let disk_0 = include_bytes!("../../../libstorage/test/gpt_test_1.bin");
        let disk_1 = include_bytes!("../../../libstorage/test/gpt_test_2.bin");
        storage.add_gpt_device(disk_0);
        storage.add_gpt_device(disk_1);
        let mut gbl_ops = FakeGblOps::new(&storage);
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);

        let off = 512;
        let size = 512;
        check_blk_upload(&mut gbl_fb, 0, off, size, disk_0);
        check_blk_upload(&mut gbl_fb, 1, off, size, disk_1);
    }

    /// A helper for testing uploading GPT partition. It verifies that data read from GPT partition
    /// `part` at disk `blk_id` in range [`off`, `off`+`size`) is the same as
    /// `partition_data[off..][..size]`.
    fn check_part_upload(
        fb: &mut impl FastbootImplementation,
        part: &str,
        off: u64,
        size: u64,
        blk_id: Option<u64>,
        partition_data: &[u8],
    ) {
        let expected =
            partition_data[off.try_into().unwrap()..][..size.try_into().unwrap()].to_vec();
        let blk_id = blk_id.map_or("".to_string(), |v| format!("{:#x}", v));
        // offset/size as part of the partition string.
        let gpt_part = format!("{}/{}/{:#x}/{:#x}", part, blk_id, off, size);
        assert_eq!(fetch(fb, gpt_part, 0, size).unwrap(), expected);
        // offset/size as separate fetch arguments.
        let gpt_part = format!("{}/{}", part, blk_id);
        assert_eq!(fetch(fb, gpt_part, off, size).unwrap(), expected);
    }

    #[test]
    fn test_fetch_partition() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 1]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_2.bin"));
        storage.add_raw_device(c"raw_0", [0xaau8; KiB!(4)]);
        storage.add_raw_device(c"raw_1", [0x55u8; KiB!(8)]);
        let mut gbl_ops = FakeGblOps::new(&storage);
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);

        let expect_boot_a = include_bytes!("../../../libstorage/test/boot_a.bin");
        let expect_boot_b = include_bytes!("../../../libstorage/test/boot_b.bin");
        let expect_vendor_boot_a = include_bytes!("../../../libstorage/test/vendor_boot_a.bin");
        let expect_vendor_boot_b = include_bytes!("../../../libstorage/test/vendor_boot_b.bin");

        let size = 512;
        let off = 512;

        check_part_upload(&mut gbl_fb, "boot_a", off, size, Some(0), expect_boot_a);
        check_part_upload(&mut gbl_fb, "boot_b", off, size, Some(0), expect_boot_b);
        check_part_upload(&mut gbl_fb, "vendor_boot_a", off, size, Some(1), expect_vendor_boot_a);
        check_part_upload(&mut gbl_fb, "vendor_boot_b", off, size, Some(1), expect_vendor_boot_b);
        check_part_upload(&mut gbl_fb, "raw_0", off, size, Some(2), &[0xaau8; KiB!(4)]);
        check_part_upload(&mut gbl_fb, "raw_1", off, size, Some(3), &[0x55u8; KiB!(8)]);

        // No block device id
        check_part_upload(&mut gbl_fb, "boot_a", off, size, None, expect_boot_a);
        check_part_upload(&mut gbl_fb, "boot_b", off, size, None, expect_boot_b);
        check_part_upload(&mut gbl_fb, "vendor_boot_a", off, size, None, expect_vendor_boot_a);
        check_part_upload(&mut gbl_fb, "vendor_boot_b", off, size, None, expect_vendor_boot_b);
        check_part_upload(&mut gbl_fb, "raw_0", off, size, None, &[0xaau8; KiB!(4)]);
        check_part_upload(&mut gbl_fb, "raw_1", off, size, None, &[0x55u8; KiB!(8)]);
    }

    /// A helper function to get a bit-flipped copy of the input data.
    fn flipped_bits(data: &[u8]) -> Vec<u8> {
        data.iter().map(|v| !(*v)).collect::<Vec<_>>()
    }

    /// A helper function to flash data to a partition
    fn flash_part(fb: &mut impl FastbootImplementation, part: &str, data: &[u8]) {
        // Prepare a download buffer.
        let download = data.to_vec();
        let resp: TestResponder = Default::default();
        set_download(fb, &download[..]);
        block_on(fb.flash(part, &resp)).unwrap();
    }

    /// A helper for testing multi partition flashing.
    fn check_flash_multi_part(
        fb: &mut impl FastbootImplementation,
        part: &str,
        actual_parts: &[&str],
        expected: &[u8],
    ) {
        flash_part(fb, part, expected);
        assert!(actual_parts
            .iter()
            .all(|v| fetch(fb, (*v).into(), 0, expected.len()).unwrap() == *expected));

        // Also flashes bit-wise reversed version in case the initial content is the same.
        let expected = &flipped_bits(expected);
        flash_part(fb, part, expected);
        assert!(actual_parts
            .iter()
            .all(|v| fetch(fb, (*v).into(), 0, expected.len()).unwrap() == *expected));
    }

    /// A helper for testing single partition flashing.
    fn check_flash_part(fb: &mut impl FastbootImplementation, part: &str, expected: &[u8]) {
        check_flash_multi_part(fb, part, &[part], expected);
    }

    #[test]
    fn test_flash_partition() {
        let disk_0 = include_bytes!("../../../libstorage/test/gpt_test_1.bin");
        let disk_1 = include_bytes!("../../../libstorage/test/gpt_test_2.bin");
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 1]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(disk_0);
        storage.add_gpt_device(disk_1);
        storage.add_raw_device(c"raw_0", [0xaau8; KiB!(4)]);
        storage.add_raw_device(c"raw_1", [0x55u8; KiB!(8)]);
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.current_slot = Some(Ok(1));
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);

        let expect_boot_a = include_bytes!("../../../libstorage/test/boot_a.bin");
        let expect_boot_b = include_bytes!("../../../libstorage/test/boot_b.bin");
        check_flash_part(&mut gbl_fb, "boot_a", expect_boot_a);
        check_flash_part(&mut gbl_fb, "boot_b", expect_boot_b);
        check_flash_part(&mut gbl_fb, "raw_0", &[0xaau8; KiB!(4)]);
        check_flash_part(&mut gbl_fb, "raw_1", &[0x55u8; KiB!(8)]);
        check_flash_part(&mut gbl_fb, "/0", disk_0);
        check_flash_part(&mut gbl_fb, "/1", disk_1);
        check_flash_multi_part(&mut gbl_fb, "boot_ab", &["boot_a", "boot_b"], expect_boot_a);
        check_flash_multi_part(&mut gbl_fb, "boot", &["boot_b"], expect_boot_a);

        // Partital flash
        let range = 0x200..0x400;
        check_flash_part(&mut gbl_fb, "boot_a//200", &expect_boot_a[range.clone()]);
        check_flash_part(&mut gbl_fb, "boot_b//200", &expect_boot_b[range.clone()]);
        // Maps to all slots
        check_flash_multi_part(
            &mut gbl_fb,
            "boot_ab//200",
            &["boot_a//200", "boot_b//200"],
            &expect_boot_a[range.clone()],
        );
        // Maps to current slot
        check_flash_multi_part(
            &mut gbl_fb,
            "boot//200",
            &["boot_b//200"],
            &expect_boot_a[range.clone()],
        );
        check_flash_part(&mut gbl_fb, "/0/200", &disk_0[range.clone()]);
        check_flash_part(&mut gbl_fb, "/1/200", &disk_1[range.clone()]);
    }

    #[test]
    fn test_flash_partition_sparse() {
        let raw = include_bytes!("../../testdata/sparse_test_raw.bin");
        let sparse = include_bytes!("../../testdata/sparse_test.bin");
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 1]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"raw", vec![0u8; raw.len()]);
        storage.add_raw_device(c"raw_0_a", vec![0u8; raw.len()]);
        storage.add_raw_device(c"raw_0_b", vec![0u8; raw.len()]);
        storage.add_raw_device(c"raw_1_b", vec![0u8; raw.len()]);
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.current_slot = Some(Ok(1));
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);

        let download = sparse.to_vec();
        let resp: TestResponder = Default::default();
        set_download(&mut gbl_fb, &download[..]);
        block_on(gbl_fb.flash("/0", &resp)).unwrap();
        assert_eq!(fetch(&mut gbl_fb, "/0".into(), 0, raw.len()).unwrap(), raw);

        // Maps to both slots
        set_download(&mut gbl_fb, &download[..]);
        block_on(gbl_fb.flash("raw_0_ab", &resp)).unwrap();
        assert_eq!(fetch(&mut gbl_fb, "raw_0_a".into(), 0, raw.len()).unwrap(), raw);
        assert_eq!(fetch(&mut gbl_fb, "raw_0_b".into(), 0, raw.len()).unwrap(), raw);
        // Maps to current slot.
        set_download(&mut gbl_fb, &download[..]);
        block_on(gbl_fb.flash("raw_1", &resp)).unwrap();
        assert_eq!(fetch(&mut gbl_fb, "raw_1_b".into(), 0, raw.len()).unwrap(), raw);
    }

    #[test]
    fn test_flash_partition_ignore_slot_suffix_with_exact_match() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 1]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_ab", [0xaau8; KiB!(4)]);
        storage.add_raw_device(c"boot", [0xaau8; KiB!(4)]);
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.current_slot = Some(Ok(1));
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);

        // It should not attempt to write boot_a or boot_b partitions since there is a partition
        // of exact match.
        check_flash_part(&mut gbl_fb, "boot_ab", &[0x55u8; KiB!(4)]);
        check_flash_part(&mut gbl_fb, "boot", &[0x55u8; KiB!(4)]);
    }

    /// A helper to invoke OEM commands.
    ///
    /// Returns the result and INFO strings.
    async fn oem(
        fb: &mut impl FastbootImplementation,
        oem_cmd: &str,
        resp: impl InfoSender + OkaySender + FailSender,
    ) -> CommandResult<()> {
        fb.oem(oem_cmd, resp).await?;
        Ok(())
    }

    #[test]
    fn test_async_flash() {
        // Creates two block devices for writing raw and sparse image.
        let sparse_raw = include_bytes!("../../testdata/sparse_test_raw.bin");
        let sparse = include_bytes!("../../testdata/sparse_test.bin");
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        storage.add_gpt_device(vec![0u8; sparse_raw.len() + 67 * 512]);
        let mut gpt_builder = storage[1].gpt_builder().unwrap();
        gpt_builder.add("sparse", [1u8; GPT_GUID_LEN], [1u8; GPT_GUID_LEN], 0, None).unwrap();
        block_on(gpt_builder.persist()).unwrap();
        let mut gbl_ops = FakeGblOps::new(&storage);
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 2]);
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);
        let tasks = gbl_fb.tasks();
        let resp: TestResponder = Default::default();

        // "oem gbl-sync-tasks" should return immediately when there is no pending IOs.
        assert!(poll(&mut pin!(oem(&mut gbl_fb, "gbl-sync-tasks", &resp))).unwrap().is_ok());
        // Enable async IO.
        assert!(poll(&mut pin!(oem(&mut gbl_fb, "gbl-enable-async-task", &resp))).unwrap().is_ok());

        // Flashes "boot_a".
        let expect_boot_a = flipped_bits(include_bytes!("../../../libstorage/test/boot_a.bin"));
        set_download(&mut gbl_fb, expect_boot_a.as_slice());
        block_on(gbl_fb.flash("boot_a", &resp)).unwrap();
        check_var(&mut gbl_fb, "block-device", "0:status", "IO pending");

        // Flashes the "sparse" partition on the different block device.
        set_download(&mut gbl_fb, sparse);
        block_on(gbl_fb.flash("sparse", &resp)).unwrap();
        check_var(&mut gbl_fb, "block-device", "1:status", "IO pending");

        {
            // "oem gbl-sync-tasks" should block.
            let oem_sync_blk_fut = &mut pin!(oem(&mut gbl_fb, "gbl-sync-tasks", &resp));
            assert!(poll(oem_sync_blk_fut).is_none());
            // Schedules the disk IO tasks to completion.
            tasks.borrow_mut().run();
            // "oem gbl-sync-tasks" should now be able to finish.
            assert!(poll(oem_sync_blk_fut).unwrap().is_ok());
        }

        // The two blocks should be in the idle state.
        check_var(&mut gbl_fb, "block-device", "0:status", "idle");
        check_var(&mut gbl_fb, "block-device", "1:status", "idle");

        // Verifies flashed image.
        assert_eq!(
            fetch(&mut gbl_fb, "boot_a".into(), 0, expect_boot_a.len()).unwrap(),
            expect_boot_a
        );
        assert_eq!(fetch(&mut gbl_fb, "sparse".into(), 0, sparse_raw.len()).unwrap(), sparse_raw);
    }

    #[test]
    fn test_async_flash_block_on_busy_blk() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 2]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_2.bin"));
        let mut gbl_ops = FakeGblOps::new(&storage);
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);
        let tasks = gbl_fb.tasks();
        let resp: TestResponder = Default::default();

        // Enable async IO.
        assert!(poll(&mut pin!(oem(&mut gbl_fb, "gbl-enable-async-task", &resp))).unwrap().is_ok());

        // Flashes boot_a partition.
        let expect_boot_a = flipped_bits(include_bytes!("../../../libstorage/test/boot_a.bin"));
        set_download(&mut gbl_fb, expect_boot_a.as_slice());
        block_on(gbl_fb.flash("boot_a", &resp)).unwrap();

        // Flashes boot_b partition.
        let expect_boot_b = flipped_bits(include_bytes!("../../../libstorage/test/boot_b.bin"));
        set_download(&mut gbl_fb, expect_boot_b.as_slice());
        {
            let flash_boot_b_fut = &mut pin!(gbl_fb.flash("boot_b", &resp));
            // Previous IO has not completed. Block is busy.
            assert!(poll(flash_boot_b_fut).is_none());
            // There should only be the previous disk IO task for "boot_a".
            assert_eq!(tasks.borrow_mut().size(), 1);
            // Schedule the disk IO task for "flash boot_a" to completion.
            tasks.borrow_mut().run();
            // The blocked "flash boot_b" should now be able to finish.
            assert!(poll(flash_boot_b_fut).is_some());
            // There should be a disk IO task spawned for "flash boot_b".
            assert_eq!(tasks.borrow_mut().size(), 1);
            // Schedule the disk IO tasks for "flash boot_b" to completion.
            tasks.borrow_mut().run();
        }

        // Verifies flashed image.
        assert_eq!(
            fetch(&mut gbl_fb, "boot_a".into(), 0, expect_boot_a.len()).unwrap(),
            expect_boot_a
        );
        assert_eq!(
            fetch(&mut gbl_fb, "boot_b".into(), 0, expect_boot_b.len()).unwrap(),
            expect_boot_b
        );
    }

    #[test]
    #[should_panic(
        expected = "A Fastboot async task failed: Other(Some(\"test\")), context: flash:boot_a"
    )]
    fn test_async_flash_error() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 2]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        let mut gbl_ops = FakeGblOps::new(&storage);
        // Injects an error.
        storage[0].partition_io(None).unwrap().dev().io().error =
            liberror::Error::Other(Some("test")).into();
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);
        let tasks = gbl_fb.tasks();
        let resp: TestResponder = Default::default();

        // Enable async IO.
        assert!(poll(&mut pin!(oem(&mut gbl_fb, "gbl-enable-async-task", &resp))).unwrap().is_ok());
        // Flashes boot_a partition.
        let expect_boot_a = flipped_bits(include_bytes!("../../../libstorage/test/boot_a.bin"));
        set_download(&mut gbl_fb, expect_boot_a.as_slice());
        block_on(gbl_fb.flash("boot_a", &resp)).unwrap();
        // Schedules the disk IO tasks to completion.
        tasks.borrow_mut().run();
    }

    #[test]
    fn test_async_erase() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 2]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"raw_0", [0xaau8; 4096]);
        storage.add_raw_device(c"raw_1", [0x55u8; 4096]);
        let mut gbl_ops = FakeGblOps::new(&storage);
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);
        let tasks = gbl_fb.tasks();
        let resp: TestResponder = Default::default();

        // Enable async IO.
        assert!(poll(&mut pin!(oem(&mut gbl_fb, "gbl-enable-async-task", &resp))).unwrap().is_ok());

        // Erases "raw_0".
        block_on(gbl_fb.erase("raw_0", &resp)).unwrap();
        check_var(&mut gbl_fb, "block-device", "0:status", "IO pending");

        // Erases second half of "raw_1"
        block_on(gbl_fb.erase("raw_1//800", &resp)).unwrap();
        check_var(&mut gbl_fb, "block-device", "1:status", "IO pending");

        {
            // "oem gbl-sync-tasks" should block.
            let oem_sync_blk_fut = &mut pin!(oem(&mut gbl_fb, "gbl-sync-tasks", &resp));
            assert!(poll(oem_sync_blk_fut).is_none());
            // Schedules the disk IO tasks to completion.
            tasks.borrow_mut().run();
            // "oem gbl-sync-tasks" should now be able to finish.
            assert!(poll(oem_sync_blk_fut).unwrap().is_ok());
        }

        // The two blocks should be in the idle state.
        check_var(&mut gbl_fb, "block-device", "0:status", "idle");
        check_var(&mut gbl_fb, "block-device", "1:status", "idle");

        // The mock storage device erases data by flipping bits. Thus 0x55 <--> 0xaa.
        assert_eq!(storage[0].partition_io(None).unwrap().dev().io().storage, [0x55u8; 4096]);
        assert_eq!(
            storage[1].partition_io(None).unwrap().dev().io().storage,
            [[0x55u8; 2048], [0xaau8; 2048]].concat()
        );
    }

    #[test]
    fn test_async_erase_unsupported_fallback_to_zeroize() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 2]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"raw_0", [0xaau8; 4096]);
        storage[0].get_blk_io().unwrap().error = Some(Error::Unsupported);
        let mut gbl_ops = FakeGblOps::new(&storage);
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);
        let resp: TestResponder = Default::default();
        // Erases "raw_0".
        block_on(gbl_fb.erase("raw_0", &resp)).unwrap();
        assert_eq!(storage[0].partition_io(None).unwrap().dev().io().storage, [0u8; 4096]);
    }

    #[test]
    #[should_panic(
        expected = "A Fastboot async task failed: Other(Some(\"test\")), context: erase:boot_a"
    )]
    fn test_async_erase_error() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 2]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        let mut gbl_ops = FakeGblOps::new(&storage);
        // Injects an error.
        storage[0].get_blk_io().unwrap().error = Some(Error::Other(Some("test")));
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);
        let tasks = gbl_fb.tasks();
        let resp: TestResponder = Default::default();

        // Enable async IO.
        assert!(poll(&mut pin!(oem(&mut gbl_fb, "gbl-enable-async-task", &resp))).unwrap().is_ok());
        // Erases boot_a partition.
        block_on(gbl_fb.erase("boot_a", &resp)).unwrap();
        // Schedules the disk IO tasks to completion.
        tasks.borrow_mut().run();
    }

    #[test]
    fn test_erase_multi_parts() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 2]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"raw_0_a", [0xaau8; KiB!(4)]);
        storage.add_raw_device(c"raw_0_b", [0xaau8; KiB!(8)]);
        storage.add_raw_device(c"raw_1_b", [0x55u8; KiB!(12)]);
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.current_slot = Some(Ok(1));
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);
        let resp: TestResponder = Default::default();

        // Erases "raw_0_a/b"
        block_on(gbl_fb.erase("raw_0_ab", &resp)).unwrap();
        block_on(gbl_fb.erase("raw_1", &resp)).unwrap();

        // The mock storage device erases data by flipping bits. Thus 0x55 <--> 0xaa.
        assert_eq!(storage[0].partition_io(None).unwrap().dev().io().storage, [0x55u8; KiB!(4)]);
        assert_eq!(storage[1].partition_io(None).unwrap().dev().io().storage, [0x55u8; KiB!(8)]);
        assert_eq!(storage[2].partition_io(None).unwrap().dev().io().storage, [0xaau8; KiB!(12)]);

        // Tests with additional offset/size
        block_on(gbl_fb.erase("raw_0_ab//400/400", &resp)).unwrap();
        block_on(gbl_fb.erase("raw_1//400/400", &resp)).unwrap();
        assert_eq!(
            storage[0].partition_io(None).unwrap().dev().io().storage,
            [vec![0x55u8; KiB!(1)], vec![0xaau8; KiB!(1)], vec![0x55u8; KiB!(2)]].concat()
        );
        assert_eq!(
            storage[1].partition_io(None).unwrap().dev().io().storage,
            [vec![0x55u8; KiB!(1)], vec![0xaau8; KiB!(1)], vec![0x55u8; KiB!(6)]].concat()
        );
        assert_eq!(
            storage[2].partition_io(None).unwrap().dev().io().storage,
            [vec![0xaau8; KiB!(1)], vec![0x55u8; KiB!(1)], vec![0xaau8; KiB!(10)]].concat()
        );
    }

    #[test]
    fn test_default_block() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 1]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        let disk_dup = include_bytes!("../../../libstorage/test/gpt_test_2.bin");
        storage.add_gpt_device(disk_dup);
        storage.add_gpt_device(disk_dup);
        let raw_a = [0xaau8; KiB!(4)];
        let raw_b = [0x55u8; KiB!(8)];
        storage.add_raw_device(c"raw", raw_a);
        storage.add_raw_device(c"raw", raw_b);
        let mut gbl_ops = FakeGblOps::new(&storage);
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);
        let resp: TestResponder = Default::default();

        let boot_a = include_bytes!("../../../libstorage/test/boot_a.bin");
        // Flips the bits on partition "vendor_boot_a" on block device #2 to make it different from
        // block #1.
        let vendor_boot_a =
            flipped_bits(include_bytes!("../../../libstorage/test/vendor_boot_a.bin"));
        flash_part(&mut gbl_fb, "vendor_boot_a/2", &vendor_boot_a);

        let size = 512;
        let off = 512;

        check_var(&mut gbl_fb, "gbl-default-block", "", "None");
        // Sets default block to #2
        block_on(oem(&mut gbl_fb, "gbl-set-default-block 2", &resp)).unwrap();
        check_var(&mut gbl_fb, "gbl-default-block", "", "0x2");
        // The following fetch should succeed and fetch from "vendor_boot_a" on block 2.
        check_part_upload(&mut gbl_fb, "vendor_boot_a", off, size, None, &vendor_boot_a);

        // Sets default block to #4 (raw_b)
        block_on(oem(&mut gbl_fb, "gbl-set-default-block 4", &resp)).unwrap();
        check_var(&mut gbl_fb, "gbl-default-block", "", "0x4");
        // The following fetch should succeed and fetch from "raw" on block 4.
        check_part_upload(&mut gbl_fb, "raw", off, size, None, &raw_b);

        // Fetches with explicit storage ID shouldn't be affected.
        check_part_upload(&mut gbl_fb, "boot_a", off, size, Some(0), boot_a);
        check_part_upload(&mut gbl_fb, "raw", off, size, Some(3), &raw_a);
        check_blk_upload(&mut gbl_fb, 1, off, size, disk_dup);

        // Fetching without storage ID should use default ID and thus the following should fail.
        assert!(fetch(&mut gbl_fb, "boot_a".into(), 0, boot_a.len()).is_err());

        // Sets default block to #1 (unmodified `disk_dup`)
        block_on(oem(&mut gbl_fb, "gbl-set-default-block 1", &resp)).unwrap();
        check_var(&mut gbl_fb, "gbl-default-block", "", "0x1");
        // Fetches whole raw block but without block ID should use the default block.
        check_part_upload(&mut gbl_fb, "", off, size, None, disk_dup);

        // Unset default block
        block_on(oem(&mut gbl_fb, "gbl-unset-default-block", &resp)).unwrap();
        check_var(&mut gbl_fb, "gbl-default-block", "", "None");
        // Fetching non-unique partitions should now fail.
        assert!(fetch(&mut gbl_fb, "raw".into(), 0, raw_a.len()).is_err());
        assert!(fetch(&mut gbl_fb, "vendor_boot_a".into(), 0, vendor_boot_a.len()).is_err());
        assert!(fetch(&mut gbl_fb, "/".into(), 0, 512).is_err());
    }

    #[test]
    fn test_set_default_block_invalid_arg() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 2]);
        let storage = FakeGblOpsStorage::default();
        let mut gbl_ops = FakeGblOps::new(&storage);
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);
        let resp: TestResponder = Default::default();
        // Missing block device ID.
        assert!(block_on(oem(&mut gbl_fb, "gbl-set-default-block ", &resp)).is_err());
        // Invalid block device ID.
        assert!(block_on(oem(&mut gbl_fb, "gbl-set-default-block zzz", &resp)).is_err());
        // Out of range block device ID. (We've added no block device).
        assert!(block_on(oem(&mut gbl_fb, "gbl-set-default-block 0", &resp)).is_err());
    }

    #[test]
    fn test_reboot_sync_all_blocks() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 2]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        let mut gbl_ops = FakeGblOps::new(&storage);
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);
        let tasks = gbl_fb.tasks();
        let resp: TestResponder = Default::default();

        block_on(oem(&mut gbl_fb, "gbl-enable-async-task", &resp)).unwrap();

        // Flashes "boot_a".
        let expect_boot_a = flipped_bits(include_bytes!("../../../libstorage/test/boot_a.bin"));
        set_download(&mut gbl_fb, expect_boot_a.as_slice());
        block_on(gbl_fb.flash("boot_a", &resp)).unwrap();
        // Checks initial state, okay_sent=false.
        assert!(!(*resp.okay_sent.try_lock().unwrap()));
        // Performs a reboot.
        let mut reboot_fut = pin!(gbl_fb.reboot(RebootMode::Normal, &resp));
        // There is a pending flash task. Reboot should wait.
        assert!(poll(&mut reboot_fut).is_none());
        assert!(!(*resp.okay_sent.try_lock().unwrap()));
        assert_eq!(resp.info_messages.try_lock().unwrap()[1], "Syncing storage...");
        // Schedules the disk IO tasks to completion.
        tasks.borrow_mut().run();
        // The reboot can now complete.
        assert!(poll(&mut reboot_fut).is_some());
        assert!((*resp.okay_sent.try_lock().unwrap()));
        assert_eq!(resp.info_messages.try_lock().unwrap()[2], "Rebooting...");
    }

    #[test]
    fn test_continue_sync_all_blocks() {
        let dl_buffers = Shared::from(vec![vec![0u8; KiB!(128)]; 2]);
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        let mut gbl_ops = FakeGblOps::new(&storage);
        let tasks = vec![].into();
        let parts = gbl_ops.disks();
        let boot_buffer = Default::default();
        let mut gbl_fb =
            GblFastboot::new(&mut gbl_ops, parts, Task::run, &tasks, &dl_buffers, boot_buffer);
        let tasks = gbl_fb.tasks();
        let resp: TestResponder = Default::default();

        block_on(oem(&mut gbl_fb, "gbl-enable-async-task", &resp)).unwrap();

        // Flashes "boot_a".
        let expect_boot_a = flipped_bits(include_bytes!("../../../libstorage/test/boot_a.bin"));
        set_download(&mut gbl_fb, expect_boot_a.as_slice());
        block_on(gbl_fb.flash("boot_a", &resp)).unwrap();
        // Performs a continue.
        let mut continue_fut = pin!(gbl_fb.r#continue(&resp));
        // There is a pending flash task. Continue should wait.
        assert!(poll(&mut continue_fut).is_none());
        assert!(!(*resp.okay_sent.try_lock().unwrap()));
        assert_eq!(resp.info_messages.try_lock().unwrap()[1], "Syncing storage...");
        // Schedules the disk IO tasks to completion.
        tasks.borrow_mut().run();
        // The continue can now complete.
        assert!(poll(&mut continue_fut).is_some());
    }

    /// Generates a length prefixed byte sequence.
    fn length_prefixed(data: &[u8]) -> Vec<u8> {
        [&data.len().to_be_bytes()[..], data].concat()
    }

    /// Used for a test implementation of [GblGenericTransport] and [GblTcpStream].
    #[derive(Default)]
    struct TestListener<'a> {
        transport_in_queue: VecDeque<Result<VecDeque<u8>, Error>>,
        transport_out_queue: VecDeque<Vec<u8>>,
        // Optional closure for injecting send errors.
        transport_out_err: Option<&'a mut dyn FnMut(&[u8]) -> Result<(), Error>>,

        tcp_in_queue: VecDeque<u8>,
        tcp_out_queue: VecDeque<u8>,
    }

    /// A shared [TestListener].
    #[derive(Default)]
    pub(crate) struct SharedTestListener<'a>(Mutex<TestListener<'a>>);

    impl<'a> SharedTestListener<'a> {
        /// Locks the listener
        fn lock(&self) -> MutexGuard<'_, TestListener<'a>> {
            self.0.try_lock().unwrap()
        }

        /// Adds packet to Transport input
        pub(crate) fn add_transport_input(&self, packet: &[u8]) {
            self.lock().transport_in_queue.push_back(Ok(packet.to_vec().into()));
        }

        /// Adds packet to Transport input
        pub(crate) fn add_transport_err(&self, err: Error) {
            self.lock().transport_in_queue.push_back(Err(err));
        }

        /// Adds bytes to input stream.
        pub(crate) fn add_tcp_input(&self, data: &[u8]) {
            self.lock().tcp_in_queue.append(&mut data.to_vec().into());
        }

        /// Adds a length pre-fixed bytes stream.
        pub(crate) fn add_tcp_length_prefixed_input(&self, data: &[u8]) {
            self.add_tcp_input(&length_prefixed(data));
        }

        /// Gets a copy of `Self::transport_out_queue`.
        pub(crate) fn transport_out_queue(&self) -> VecDeque<Vec<u8>> {
            self.lock().transport_out_queue.clone()
        }

        /// Gets a copy of `Self::tcp_out_queue`.
        pub(crate) fn tcp_out_queue(&self) -> VecDeque<u8> {
            self.lock().tcp_out_queue.clone()
        }

        /// A helper for decoding Transport output packets as a string
        pub(crate) fn dump_transport_out_queue(&self) -> String {
            let mut res = String::from("");
            for (i, v) in self.lock().transport_out_queue.iter().enumerate() {
                let v = match v.len() <= MAX_RESPONSE_SIZE {
                    true => {
                        format!("b{:?}", String::from_utf8(v.clone()).unwrap_or(format!("{:?}", v)))
                    }
                    _ => format!("(packet #{i}, {} bytes)", v.len()),
                };
                res += format!("{v},\n").as_str();
            }
            res
        }

        /// A helper for decoding TCP output data as strings
        pub(crate) fn dump_tcp_out_queue(&self) -> String {
            let mut data = self.lock();
            let mut v;
            let (_, mut remains) = data.tcp_out_queue.make_contiguous().split_at(4);
            let mut res = String::from("");
            while !remains.is_empty() {
                // Parses length-prefixed payload.
                let (len, rest) = remains.split_first_chunk::<{ size_of::<u64>() }>().unwrap();
                (v, remains) = rest.split_at(u64::from_be_bytes(*len).try_into().unwrap());
                let s = String::from_utf8(v.to_vec()).unwrap_or(format!("{:?}", v));
                res += format!("b{:?},\n", s).as_str();
            }
            res
        }
    }

    impl Transport for &SharedTestListener<'_> {
        async fn receive(&mut self, out: &mut [u8]) -> Result<(usize, usize), Error> {
            match self.lock().transport_in_queue.pop_front() {
                Some(Ok(mut v)) => {
                    let (sz, rem) = v.read(out).map(|s| (s, v.len())).unwrap();
                    (rem > 0).then(|| Some(self.lock().transport_in_queue.push_front(Ok(v))));
                    Ok((sz, rem))
                }
                Some(Err(e)) => Err(e),
                _ => Err(Error::Other(Some("No more data"))),
            }
        }

        async fn send_packet(&mut self, packet: &[u8]) -> Result<(), Error> {
            self.lock().transport_out_err.as_mut().map(|f| f(packet)).unwrap_or(Ok(()))?;
            Ok(self.lock().transport_out_queue.push_back(packet.into()))
        }
    }

    impl GblGenericTransport for &SharedTestListener<'_> {
        fn has_packet(&mut self) -> bool {
            !self.lock().transport_in_queue.is_empty()
        }
    }

    impl TcpStream for &SharedTestListener<'_> {
        async fn read(&mut self, out: &mut [u8]) -> Result<usize, Error> {
            match self.lock().tcp_in_queue.read(out).unwrap() {
                0 => Err(Error::Other(Some("No more data"))),
                v => Ok(v),
            }
        }

        async fn write_exact(&mut self, data: &[u8]) -> Result<(), Error> {
            Ok(self.lock().tcp_out_queue.append(&mut data.to_vec().into()))
        }
    }

    impl GblTcpStream for &SharedTestListener<'_> {
        fn accept_new(&mut self) -> bool {
            !self.lock().tcp_in_queue.is_empty()
        }
    }

    /// A helper to make an expected stream of Transport output.
    pub(crate) fn make_expected_transport_out(data: &[&[u8]]) -> VecDeque<Vec<u8>> {
        VecDeque::from(data.iter().map(|v| v.to_vec()).collect::<Vec<_>>())
    }

    /// A helper to make an expected stream of TCP output.
    fn make_expected_tcp_out(data: &[&[u8]]) -> VecDeque<u8> {
        let mut res = VecDeque::<u8>::from(b"FB01".to_vec());
        data.iter().for_each(|v| res.append(&mut length_prefixed(v).into()));
        res
    }

    #[test]
    fn test_run_gbl_fastboot() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        listener.add_transport_input(b"getvar:version-bootloader");
        listener.add_tcp_input(b"FB01");
        listener.add_tcp_length_prefixed_input(b"getvar:max-download-size");
        listener.add_tcp_length_prefixed_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                format!("OKAY{}", expected_version_bootloader()).as_bytes()
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        assert_eq!(
            listener.tcp_out_queue(),
            make_expected_tcp_out(&[b"OKAY0x20000", b"INFOSyncing storage...", b"OKAY"]),
            "\nActual TCP output:\n{}",
            listener.dump_tcp_out_queue()
        );
    }

    #[test]
    fn test_run_gbl_fastboot_parallel_task() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"raw_0", [0u8; KiB!(4)]);
        storage.add_raw_device(c"raw_1", [0u8; KiB!(8)]);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        // New scope to release reference on local
        {
            let mut fb_fut = pin!(run_gbl_fastboot_stack::<3>(
                &mut gbl_ops,
                buffers,
                transports,
                Some(tcp),
                Default::default(),
            ));

            listener.add_transport_input(b"oem gbl-enable-async-task");
            listener.add_transport_input(format!("download:{:#x}", KiB!(4)).as_bytes());
            listener.add_transport_input(&[0x55u8; KiB!(4)]);
            listener.add_transport_input(b"flash:raw_0");

            listener.add_tcp_input(b"FB01");
            listener.add_tcp_length_prefixed_input(format!("download:{:#x}", KiB!(8)).as_bytes());
            listener.add_tcp_length_prefixed_input(&[0xaau8; KiB!(8)]);
            listener.add_tcp_length_prefixed_input(b"flash:raw_1");

            assert!(poll_n_times(&mut fb_fut, 100).is_none());
        }

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"OKAY",
                b"DATA00001000",
                b"OKAY",
                b"INFOAn async task is launched. To sync manually, run \"oem gbl-sync-tasks\".",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        assert_eq!(
            listener.tcp_out_queue(),
            make_expected_tcp_out(&[
                b"DATA00002000",
                b"OKAY",
                b"INFOAn async task is launched. To sync manually, run \"oem gbl-sync-tasks\".",
                b"OKAY",
            ]),
            "\nActual TCP output:\n{}",
            listener.dump_tcp_out_queue()
        );

        // Verifies flashed image on raw_0.
        assert_eq!(storage[0].partition_io(None).unwrap().dev().io().storage, [0x55u8; KiB!(4)]);

        // Verifies flashed image on raw_1.
        assert_eq!(storage[1].partition_io(None).unwrap().dev().io().storage, [0xaau8; KiB!(8)]);
    }

    #[test]
    fn test_run_gbl_fastboot_download_oversize() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(1)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        listener.add_transport_input(b"download:0x401");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"FAILBuffer too small 0x400. Needs 0x401",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    #[cfg(feature = "fuchsia")]
    fn test_oem_add_staged_bootloader_file() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.get_zbi_bootloader_files_buffer().unwrap().fill(0);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        // Stages two zbi files.
        listener.add_transport_input(format!("download:{:#x}", 3).as_bytes());
        listener.add_transport_input(b"foo");
        listener.add_transport_input(b"oem add-staged-bootloader-file file_1");
        listener.add_transport_input(format!("download:{:#x}", 3).as_bytes());
        listener.add_transport_input(b"bar");
        listener.add_transport_input(b"oem add-staged-bootloader-file file_2");
        listener.add_transport_input(b"continue");

        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        let buffer = gbl_ops.get_zbi_bootloader_files_buffer_aligned().unwrap();
        let container = ZbiContainer::parse(&buffer[..]).unwrap();
        let mut iter = container.iter();
        assert_eq!(iter.next().unwrap().payload.as_bytes(), b"\x06file_1foo");
        assert_eq!(iter.next().unwrap().payload.as_bytes(), b"\x06file_2bar");
        assert!(iter.next().is_none());
    }

    #[test]
    #[cfg(feature = "fuchsia")]
    fn test_oem_add_staged_bootloader_file_missing_file_name() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        listener.add_transport_input(format!("download:{:#x}", 3).as_bytes());
        listener.add_transport_input(b"foo");
        listener.add_transport_input(b"oem add-staged-bootloader-file");
        listener.add_transport_input(b"continue");

        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"DATA00000003",
                b"OKAY",
                b"FAILMissing file name",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        )
    }

    #[test]
    #[cfg(feature = "fuchsia")]
    fn test_oem_add_staged_bootloader_file_missing_download() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        listener.add_transport_input(b"oem add-staged-bootloader-file file1");
        listener.add_transport_input(b"continue");

        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"FAILNo file staged",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_oem_vendor_cmd() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        listener.add_transport_input(b"download:0x1000");
        listener.add_transport_input(&[0x55u8; 0x1000]);
        listener.add_transport_input(b"oem test-oem");
        listener.add_transport_input(b"continue");

        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"DATA00001000",
                b"OKAY",
                format!("INFO{}", FakeGblOps::GBL_OEM_CMD_INFO_MSG).as_bytes(),
                b"OKAY",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_fuchsia_fastboot_mdns_packet() {
        let expected = [
            0x00, 0x00, 0x84, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x09, 0x5f,
            0x66, 0x61, 0x73, 0x74, 0x62, 0x6f, 0x6f, 0x74, 0x04, 0x5f, 0x74, 0x63, 0x70, 0x05,
            0x6c, 0x6f, 0x63, 0x61, 0x6c, 0x00, 0x00, 0x0c, 0x80, 0x01, 0x00, 0x00, 0x00, 0x78,
            0x00, 0x19, 0x16, 0x66, 0x75, 0x63, 0x68, 0x73, 0x69, 0x61, 0x2d, 0x35, 0x32, 0x35,
            0x34, 0x2d, 0x30, 0x30, 0x31, 0x32, 0x2d, 0x33, 0x34, 0x35, 0x36, 0xc0, 0x0c, 0xc0,
            0x2c, 0x00, 0x21, 0x80, 0x01, 0x00, 0x00, 0x00, 0x78, 0x00, 0x1f, 0x00, 0x00, 0x00,
            0x00, 0x15, 0xb2, 0x16, 0x66, 0x75, 0x63, 0x68, 0x73, 0x69, 0x61, 0x2d, 0x35, 0x32,
            0x35, 0x34, 0x2d, 0x30, 0x30, 0x31, 0x32, 0x2d, 0x33, 0x34, 0x35, 0x36, 0xc0, 0x1b,
            0xc0, 0x57, 0x00, 0x1c, 0x80, 0x01, 0x00, 0x00, 0x00, 0x78, 0x00, 0x10, 0xfe, 0x80,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x50, 0x54, 0x00, 0xff, 0xfe, 0x12, 0x34, 0x56,
        ];
        let ip6_addr = &[
            0xfe, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x50, 0x54, 0x00, 0xff, 0xfe, 0x12,
            0x34, 0x56,
        ];
        assert_eq!(
            fuchsia_fastboot_mdns_packet("fuchsia-5254-0012-3456", ip6_addr).unwrap(),
            expected
        );
    }

    #[test]
    fn test_fuchsia_fastboot_mdns_packet_invalid_node_name() {
        let ip6_addr = &[
            0xfe, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x50, 0x54, 0x00, 0xff, 0xfe, 0x12,
            0x34, 0x56,
        ];
        assert!(fuchsia_fastboot_mdns_packet("fuchsia-5254-0012-345", ip6_addr).is_err());
        assert!(fuchsia_fastboot_mdns_packet("fuchsia-5254-0012-34567", ip6_addr).is_err());
    }

    #[test]
    fn test_update_gpt() {
        let disk_orig = include_bytes!("../../../libstorage/test/gpt_test_1.bin");
        // Erase the primary and secondary header.
        let mut disk = disk_orig.to_vec();
        disk[512..][..512].fill(0);
        disk.last_chunk_mut::<512>().unwrap().fill(0);

        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(&disk);
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_2.bin"));
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut load_buffer = AlignedBuffer::new(MiB!(8), KERNEL_ALIGNMENT);
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        // Checks that there is no valid partitions for block #0.
        listener.add_transport_input(b"getvar:partition-size:boot_a");
        listener.add_transport_input(b"getvar:partition-size:boot_b");
        // No partitions on block #0 should show up in `getvar:all` despite being a GPT device,
        // since the GPTs are corrupted.
        listener.add_transport_input(b"getvar:all");
        // Download a GPT
        let gpt = &disk_orig[..34 * 512];
        listener.add_transport_input(format!("download:{:#x}", gpt.len()).as_bytes());
        listener.add_transport_input(gpt);
        listener.add_transport_input(b"flash:gpt/0");
        // Checks that we can get partition info now.
        listener.add_transport_input(b"getvar:partition-size:boot_a");
        listener.add_transport_input(b"getvar:partition-size:boot_b");
        listener.add_transport_input(b"getvar:all");

        listener.add_transport_input(b"continue");

        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            GblFbData { boot_buffer: load_buffer.as_mut().into(), ..Default::default() },
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"FAILNotFound",
                b"FAILNotFound",
                b"INFOmax-download-size: 0x20000",
                format!("INFOversion-bootloader: {}", expected_version_bootloader()).as_bytes(),
                b"INFOslot-count: 2",
                b"INFOcurrent-slot: a",
                b"INFOslot-successful:a: no",
                b"INFOslot-unbootable:a: no",
                b"INFOslot-retry-count:a: 7",
                b"INFOslot-successful:b: no",
                b"INFOslot-unbootable:b: no",
                b"INFOslot-retry-count:b: 7",
                b"INFOhas-slot:vendor_boot: yes",
                b"INFOmax-fetch-size: 0x7fffffff",
                b"INFOblock-device:0:total-blocks: 0x80",
                b"INFOblock-device:0:block-size: 0x200",
                b"INFOblock-device:0:status: idle",
                b"INFOblock-device:1:total-blocks: 0x100",
                b"INFOblock-device:1:block-size: 0x200",
                b"INFOblock-device:1:status: idle",
                b"INFOgbl-default-block: None",
                b"INFOpartition-size:vendor_boot_a: 0x1000",
                b"INFOpartition-type:vendor_boot_a: raw",
                b"INFOpartition-guid:vendor_boot_a: 42aaac2e-37e3-43ba-9930-42dfa96e6334",
                b"INFOpartition-size:vendor_boot_b: 0x1800",
                b"INFOpartition-type:vendor_boot_b: raw",
                b"INFOpartition-guid:vendor_boot_b: bdadfeca-879c-43e9-8f0d-8ef7da29b5e7",
                b"INFOunlocked: no",
                b"INFOunlocked-critical: no",
                format!("INFO{}:1: {}:1", FakeGblOps::GBL_TEST_VAR, FakeGblOps::GBL_TEST_VAR_VAL)
                    .as_bytes(),
                format!("INFO{}:2: {}:2", FakeGblOps::GBL_TEST_VAR, FakeGblOps::GBL_TEST_VAR_VAL)
                    .as_bytes(),
                format!(
                    "INFO{}: {}",
                    FakeGblOps::GBL_TEST_VAR_UNSPLIT,
                    FakeGblOps::GBL_TEST_VAR_UNSPLIT_VAL
                )
                .as_bytes(),
                b"OKAY",
                b"DATA00004400",
                b"OKAY",
                b"INFOUpdating GPT...",
                b"OKAY",
                b"OKAY0x2000",
                b"OKAY0x3000",
                b"INFOmax-download-size: 0x20000",
                format!("INFOversion-bootloader: {}", expected_version_bootloader()).as_bytes(),
                b"INFOslot-count: 2",
                b"INFOcurrent-slot: a",
                b"INFOslot-successful:a: no",
                b"INFOslot-unbootable:a: no",
                b"INFOslot-retry-count:a: 7",
                b"INFOslot-successful:b: no",
                b"INFOslot-unbootable:b: no",
                b"INFOslot-retry-count:b: 7",
                b"INFOhas-slot:boot: yes",
                b"INFOhas-slot:vendor_boot: yes",
                b"INFOmax-fetch-size: 0x7fffffff",
                b"INFOblock-device:0:total-blocks: 0x80",
                b"INFOblock-device:0:block-size: 0x200",
                b"INFOblock-device:0:status: idle",
                b"INFOblock-device:1:total-blocks: 0x100",
                b"INFOblock-device:1:block-size: 0x200",
                b"INFOblock-device:1:status: idle",
                b"INFOgbl-default-block: None",
                b"INFOpartition-size:boot_a: 0x2000",
                b"INFOpartition-type:boot_a: raw",
                b"INFOpartition-guid:boot_a: 42aaac2e-37e3-43ba-9930-42dfa96e6334",
                b"INFOpartition-size:boot_b: 0x3000",
                b"INFOpartition-type:boot_b: raw",
                b"INFOpartition-guid:boot_b: bdadfeca-879c-43e9-8f0d-8ef7da29b5e7",
                b"INFOpartition-size:vendor_boot_a: 0x1000",
                b"INFOpartition-type:vendor_boot_a: raw",
                b"INFOpartition-guid:vendor_boot_a: 42aaac2e-37e3-43ba-9930-42dfa96e6334",
                b"INFOpartition-size:vendor_boot_b: 0x1800",
                b"INFOpartition-type:vendor_boot_b: raw",
                b"INFOpartition-guid:vendor_boot_b: bdadfeca-879c-43e9-8f0d-8ef7da29b5e7",
                b"INFOunlocked: no",
                b"INFOunlocked-critical: no",
                format!("INFO{}:1: {}:1", FakeGblOps::GBL_TEST_VAR, FakeGblOps::GBL_TEST_VAR_VAL)
                    .as_bytes(),
                format!("INFO{}:2: {}:2", FakeGblOps::GBL_TEST_VAR, FakeGblOps::GBL_TEST_VAR_VAL)
                    .as_bytes(),
                format!(
                    "INFO{}: {}",
                    FakeGblOps::GBL_TEST_VAR_UNSPLIT,
                    FakeGblOps::GBL_TEST_VAR_UNSPLIT_VAL
                )
                .as_bytes(),
                b"OKAY",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_update_gpt_resize() {
        let disk_orig = include_bytes!("../../../libstorage/test/gpt_test_1.bin");
        let mut disk = disk_orig.to_vec();
        // Doubles the size of the disk
        disk.resize(disk_orig.len() * 2, 0);

        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_2.bin"));
        storage.add_gpt_device(&disk);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        // Checks current size of last partition `boot_b`.
        listener.add_transport_input(b"getvar:partition-size:boot_b");
        // Sets a default block.
        listener.add_transport_input(b"oem gbl-set-default-block 1");
        let gpt = &disk_orig[..34 * 512];
        listener.add_transport_input(format!("download:{:#x}", gpt.len()).as_bytes());
        listener.add_transport_input(gpt);
        // No need to specify block device index
        listener.add_transport_input(b"flash:gpt//resize");
        // Checks updated size of last partition `boot_b`.
        listener.add_transport_input(b"getvar:partition-size:boot_b");
        listener.add_transport_input(b"continue");

        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"OKAY0x3000",
                b"INFODefault block device: 0x1",
                b"OKAY",
                b"DATA00004400",
                b"OKAY",
                b"INFOUpdating GPT...",
                b"OKAY",
                b"OKAY0x15a00",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_update_gpt_no_downloaded_gpt() {
        let disk = include_bytes!("../../../libstorage/test/gpt_test_1.bin");
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(&disk);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        listener.add_transport_input(b"flash:gpt/0");
        listener.add_transport_input(b"continue");

        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"FAILNo GPT downloaded",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_update_gpt_bad_gpt() {
        let disk = include_bytes!("../../../libstorage/test/gpt_test_1.bin");
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(&disk);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        // Download a bad GPT.
        let mut gpt = disk[..34 * 512].to_vec();
        gpt[512] = !gpt[512];
        listener.add_transport_input(format!("download:{:#x}", gpt.len()).as_bytes());
        listener.add_transport_input(&gpt);
        listener.add_transport_input(b"flash:gpt/0");
        listener.add_transport_input(b"continue");

        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"DATA00004400",
                b"OKAY",
                b"INFOUpdating GPT...",
                b"FAILGptError(\n    IncorrectMagic(\n        6075990659671082682,\n    ),\n)",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_update_gpt_invalid_input() {
        let disk_orig = include_bytes!("../../../libstorage/test/gpt_test_1.bin");
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(&disk_orig);
        storage.add_gpt_device(&disk_orig);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        let gpt = &disk_orig[..34 * 512];
        listener.add_transport_input(format!("download:{:#x}", gpt.len()).as_bytes());
        listener.add_transport_input(gpt);
        // Missing block device ID.
        listener.add_transport_input(b"flash:gpt");
        // Out of range block device ID.
        listener.add_transport_input(b"flash:gpt/2");
        // Invalid option.
        listener.add_transport_input(b"flash:gpt/0/invalid-arg");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"DATA00004400",
                b"OKAY",
                b"FAILBlock ID is required for flashing GPT",
                b"FAILInvalid block ID",
                b"FAILUnknown argument",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_update_gpt_fail_on_raw_blk() {
        let disk_orig = include_bytes!("../../../libstorage/test/gpt_test_1.bin");
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"raw_0", vec![0u8; KiB!(1024)]);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        let gpt = &disk_orig[..34 * 512];
        listener.add_transport_input(format!("download:{:#x}", gpt.len()).as_bytes());
        listener.add_transport_input(gpt);
        listener.add_transport_input(b"flash:gpt/0");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"DATA00004400",
                b"OKAY",
                b"INFOUpdating GPT...",
                b"FAILBlock device is not for GPT",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_oem_erase_gpt() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_2.bin"));
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        // Erases the GPT on disk #0.
        listener.add_transport_input(b"erase:gpt/0");
        // Checks that we can no longer get partition info on disk #0.
        listener.add_transport_input(b"getvar:partition-size:boot_a");
        listener.add_transport_input(b"getvar:partition-size:boot_b");
        // Checks that we can still get partition info on disk #1.
        listener.add_transport_input(b"getvar:partition-size:vendor_boot_a");
        listener.add_transport_input(b"getvar:partition-size:vendor_boot_b");
        listener.add_transport_input(b"continue");

        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"OKAY",
                b"FAILNotFound",
                b"FAILNotFound",
                b"OKAY0x1000",
                b"OKAY0x1800",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_oem_erase_gpt_fail_on_raw_blk() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"raw_0", vec![0u8; KiB!(1024)]);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        listener.add_transport_input(b"erase:gpt/0");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"FAILBlock device is not for GPT",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    /// Helper for testing fastboot set_active in fuchsia A/B/R mode.
    #[cfg(feature = "fuchsia")]
    fn test_run_gbl_fastboot_set_active_fuchsia_abr(slot_ch: char, slot: SlotIndex) {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"durable_boot", [0x00u8; KiB!(4)]);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.os = Some(Os::Fuchsia);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        mark_slot_unbootable(&mut GblAbrOps(&mut gbl_ops), SlotIndex::A).unwrap();
        mark_slot_unbootable(&mut GblAbrOps(&mut gbl_ops), SlotIndex::B).unwrap();

        // Flash some data to `durable_boot` after A/B/R metadata. This is for testing that sync
        // storage is done first.
        let data = vec![0x55u8; KiB!(4) - ABR_DATA_SIZE];
        listener.add_transport_input(b"oem gbl-enable-async-task");
        listener.add_transport_input(format!("download:{:#x}", KiB!(4) - ABR_DATA_SIZE).as_bytes());
        listener.add_transport_input(&data);
        listener
            .add_transport_input(format!("flash:durable_boot//{:#x}", ABR_DATA_SIZE).as_bytes());
        // Issues set_active commands
        listener.add_transport_input(format!("set_active:{slot_ch}").as_bytes());
        listener.add_transport_input(b"continue");
        let res = block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));
        assert_eq!(res.last_set_active_slot, Some(slot_ch));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"OKAY",
                b"DATA00000fe0",
                b"OKAY",
                b"INFOAn async task is launched. To sync manually, run \"oem gbl-sync-tasks\".",
                b"OKAY",
                b"OKAY",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        assert_eq!(get_boot_slot(&mut GblAbrOps(&mut gbl_ops), true), (slot, false));
        // Verifies storage sync
        assert_eq!(
            storage[0].partition_io(None).unwrap().dev().io().storage[ABR_DATA_SIZE..],
            data
        );
    }

    #[test]
    #[cfg(feature = "fuchsia")]
    fn test_run_gbl_fastboot_set_active_fuchsia_abr_a() {
        test_run_gbl_fastboot_set_active_fuchsia_abr('a', SlotIndex::A);
    }

    #[test]
    #[cfg(feature = "fuchsia")]
    fn test_run_gbl_fastboot_set_active_fuchsia_abr_b() {
        test_run_gbl_fastboot_set_active_fuchsia_abr('b', SlotIndex::B);
    }

    #[test]
    #[cfg(feature = "fuchsia")]
    fn test_run_gbl_fastboot_set_active_fuchsia_abr_invalid_slot() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"durable_boot", [0x00u8; KiB!(4)]);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.os = Some(Os::Fuchsia);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        listener.add_transport_input(b"set_active:r");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"FAILInvalid slot index for Fuchsia A/B/R",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_run_gbl_fastboot_set_active_android() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.os = Some(Os::Android);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        listener.add_transport_input(b"getvar:current-slot");
        listener.add_transport_input(b"set_active:b");
        listener.add_transport_input(b"getvar:current-slot");
        listener.add_transport_input(b"continue");
        let res = block_on(run_gbl_fastboot_stack::<2>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"OKAYa",
                b"OKAY",
                b"OKAYb",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual USB output:\n{}",
            listener.dump_transport_out_queue()
        );
        assert_eq!(res.last_set_active_slot, Some('b'));
    }

    #[test]
    fn test_run_gbl_fastboot_set_active_multichar_slot() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"set_active:ab");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"FAILSlot suffix must be one character",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    fn test_fastboot_reboot(
        reboot_command: &[u8],
        expected_info: &[u8],
        expected_boot_mode: AndroidBootMode,
    ) {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"misc", [0u8; KiB!(4)]);
        let mut gbl_ops = FakeGblOps::new(&storage);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        let mut load_buffer = AlignedBuffer::new(MiB!(8), KERNEL_ALIGNMENT);

        listener.add_transport_input(reboot_command);
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            GblFbData { boot_buffer: (&mut load_buffer[..]).into(), ..Default::default() },
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"INFOSyncing storage...",
                expected_info,
                b"OKAY",
                b"FAILAborted",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
        let bcb = read_bootloader_message(&mut gbl_ops).unwrap();
        assert_eq!(bcb.boot_mode(), Ok(expected_boot_mode));
        assert!(gbl_ops.rebooted);
    }

    #[test]
    fn test_run_gbl_fastboot_reboot() {
        test_fastboot_reboot(b"reboot", b"INFORebooting...", AndroidBootMode::Normal);
    }

    #[test]
    fn test_run_gbl_fastboot_bootloader() {
        test_fastboot_reboot(
            b"reboot-bootloader",
            b"INFORebooting to bootloader...",
            AndroidBootMode::BootloaderBootOnce,
        );
    }

    #[test]
    fn test_run_gbl_fastboot_reboot_fastboot() {
        test_fastboot_reboot(
            b"reboot-fastboot",
            b"INFORebooting to userspace fastboot...",
            AndroidBootMode::Fastboot,
        );
    }

    #[test]
    fn test_run_gbl_fastboot_reboot_recovery() {
        test_fastboot_reboot(
            b"reboot-recovery",
            b"INFORebooting to recovery...",
            AndroidBootMode::Recovery,
        );
    }

    #[test]
    #[cfg(feature = "fuchsia")]
    fn test_run_gbl_fastboot_fuchsia_reboot_bootloader_abr() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"durable_boot", [0x00u8; KiB!(4)]);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.os = Some(Os::Fuchsia);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        listener.add_transport_input(b"reboot-bootloader");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"INFOSyncing storage...",
                b"INFORebooting to bootloader...",
                b"OKAY",
                b"FAILAborted",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        assert_eq!(get_and_clear_one_shot_bootloader(&mut GblAbrOps(&mut gbl_ops)), Ok(true));
        assert!(gbl_ops.rebooted);
    }

    #[test]
    #[cfg(feature = "fuchsia")]
    fn test_run_gbl_fastboot_fuchsia_reboot_recovery_abr() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"durable_boot", [0x00u8; KiB!(4)]);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.os = Some(Os::Fuchsia);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        listener.add_transport_input(b"reboot-recovery");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"INFOSyncing storage...",
                b"INFORebooting to recovery...",
                b"OKAY",
                b"FAILAborted",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        // One shot recovery is set.
        assert_eq!(get_boot_slot(&mut GblAbrOps(&mut gbl_ops), true), (SlotIndex::R, false));
        assert_eq!(get_boot_slot(&mut GblAbrOps(&mut gbl_ops), true), (SlotIndex::A, false));
        assert!(gbl_ops.rebooted);
    }

    #[test]
    #[cfg(feature = "fuchsia")]
    fn test_legacy_fvm_partition_alias() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"fuchsia-fvm", [0x00u8; KiB!(4)]);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.os = Some(Os::Fuchsia);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        listener.add_transport_input(format!("download:{:#x}", KiB!(4)).as_bytes());
        listener.add_transport_input(&[0xaau8; KiB!(4)]);
        listener.add_transport_input(b"flash:fvm");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"DATA00001000",
                b"OKAY",
                b"OKAY",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_async_flash_early_errors() {
        let sparse_raw = include_bytes!("../../testdata/sparse_test_raw.bin");
        let sparse = include_bytes!("../../testdata/sparse_test.bin");
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"raw", vec![0u8; sparse_raw.len() - 1]);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"oem gbl-enable-async-task");
        // Flashes an oversized image.
        listener.add_transport_input(format!("download:{:#x}", sparse_raw.len()).as_bytes());
        listener.add_transport_input(&vec![0xaau8; sparse_raw.len()]);
        listener.add_transport_input(b"flash:raw");
        // Flashes an oversized sparse image.
        listener.add_transport_input(format!("download:{:#x}", sparse.len()).as_bytes());
        listener.add_transport_input(sparse);
        listener.add_transport_input(b"flash:raw");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        // The out-of-range errors should be caught before async task is launched.
        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"OKAY",
                b"DATA0000e000",
                b"OKAY",
                b"FAILOutOfRange",
                b"DATA00006080",
                b"OKAY",
                b"FAILOutOfRange",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    fn test_fastboot_boot_slot(
        idx: u8,
        suffix: char,
        load_buffer: &mut [u8],
    ) -> (&[u8], &[u8], &[u8], &mut [u8]) {
        let mut storage = FakeGblOpsStorage::default();
        let vbmeta = CString::new(format!("vbmeta_{suffix}")).unwrap();
        let vbmeta_img = read_test_data(format!("vbmeta_v2_{suffix}.img"));
        storage.add_raw_device(&vbmeta, vbmeta_img);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = default_test_gbl_ops(&storage);
        gbl_ops.current_slot = Some(Ok(idx));
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        let data = read_test_data(format!("boot_v2_{suffix}.img"));
        listener.add_transport_input(format!("download:{:#x}", data.len()).as_bytes());
        listener.add_transport_input(&data);
        listener.add_transport_input(b"boot");
        listener.add_transport_input(b"continue");

        let res = block_on(run_gbl_fastboot_stack::<2>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            GblFbData { boot_buffer: (&mut load_buffer[..]).into(), ..Default::default() },
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"DATA00004000",
                b"OKAY",
                format!("INFOBoot image as Android slot {suffix}").as_bytes(),
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        res.split_loaded_android((&mut load_buffer[..]).into()).unwrap()
    }

    #[test]
    fn test_fastboot_boot_slot_a() {
        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        let (ramdisk, _, kernel, _) = test_fastboot_boot_slot(0, 'a', &mut load_buffer);
        checks_loaded_v2_slot_a_normal_mode(ramdisk, kernel);
    }

    #[test]
    fn test_fastboot_boot_slot_b() {
        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        let (ramdisk, _, kernel, _) = test_fastboot_boot_slot(1, 'b', &mut load_buffer);
        checks_loaded_v2_slot_b_normal_mode(ramdisk, kernel);
    }

    #[test]
    fn test_fastboot_boot_reentrant() {
        // Tests that "fastboot boot" is reentrant valid.
        let mut storage = FakeGblOpsStorage::default();
        let vbmeta = CString::new(format!("vbmeta_a")).unwrap();
        let vbmeta_img = read_test_data(format!("vbmeta_v4_v4_init_boot_a.img"));
        storage.add_raw_device(&vbmeta, vbmeta_img);

        // Use preloaded buffers so that we can test that `GblOps::get_partition_buffer()` allows
        // backend to handle buffer acquire and release.
        let preloaded = vec![
            (Partition::VendorKernelBoot, "vendor_kernel_boot_a.img"),
            (Partition::VendorBoot, "vendor_boot_v4_a.img"),
            (Partition::InitBoot, "init_boot_a.img"),
        ];
        let buffers = HashMap::<Partition, RefCell<Vec<u8>>>::from_iter(
            preloaded.iter().map(|(p, f)| (p.clone(), read_test_data(f).into())),
        );
        let get_partition_buffer_handler = |n: &Partition| {
            Ok(PartitionBuffer::Preloaded(into_refmut_bytes(
                buffers.get(n).ok_or(Error::NotFound)?.borrow_mut(),
            )))
        };

        let mut gbl_ops = default_test_gbl_ops(&storage);
        gbl_ops.get_partition_buffer_handler = Some(&get_partition_buffer_handler);
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);

        // "fastboot boot boot_v2_a.img" should failed.
        let data = read_test_data("boot_v2_a.img");
        listener.add_transport_input(format!("download:{:#x}", data.len()).as_bytes());
        listener.add_transport_input(&data);
        listener.add_transport_input(b"boot");

        // "fastboot boot boot_no_ramdisk_v4_a.img", from the same state, should succeed.
        let data = read_test_data("boot_no_ramdisk_v4_a.img");
        listener.add_transport_input(format!("download:{:#x}", data.len()).as_bytes());
        listener.add_transport_input(&data);
        listener.add_transport_input(b"boot");
        listener.add_transport_input(b"continue");

        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        let res = block_on(run_gbl_fastboot_stack::<2>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            GblFbData { boot_buffer: (&mut load_buffer[..]).into(), ..Default::default() },
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"DATA00004000",
                b"OKAY",
                b"FAILAvbSlotVerifyError(Verification(None))",
                b"DATA00002000",
                b"OKAY",
                b"INFOBoot image as Android slot a",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        let (ramdisk, _, kernel, _) =
            res.split_loaded_android((&mut load_buffer[..]).into()).unwrap();
        assert_eq!(kernel, read_test_data("kernel_a.img"));
        assert!(ramdisk.starts_with(
            &[
                read_test_data("vendor_ramdisk_a.img"),
                read_test_data("vendor_kernel_a.img"),
                read_test_data("generic_ramdisk_a.img"),
            ]
            .concat()
        ));
    }

    #[test]
    fn test_fastboot_no_channels() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = default_test_gbl_ops(&storage);
        let no_transports: &mut [&SharedTestListener] = &mut [];

        block_on(run_gbl_fastboot_stack::<2>(
            &mut gbl_ops,
            buffers,
            no_transports,
            None::<&SharedTestListener>,
            Default::default(),
        ));
    }

    #[test]
    fn test_fastboot_getvar_slot_count() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.slot_count = Some(Ok(123));
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"getvar:slot-count");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[b"OKAY123", b"INFOSyncing storage...", b"OKAY",]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_upload() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(1)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);

        // Uploads 2k of data
        let mut data: &[u8] = &vec![0x55u8; KiB!(2)];
        let handler = &mut |out: &mut [u8]| {
            let to_read = min(out.len(), data.len());
            out[..to_read].clone_from_slice(&data[..to_read]);
            data = &data[to_read..];
            Ok((to_read, data.len()))
        };
        gbl_ops.get_staged_handler = Some(handler);

        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"upload");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"INFOUploading 2048 bytes...",
                b"DATA00000800",
                &vec![0x55u8; 1024],
                &vec![0x55u8; 1024],
                b"OKAY",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_upload_recycle_download_buffer() {
        let storage = FakeGblOpsStorage::default();
        // Provides only 1 download buffer to test recycling.
        let buffers = vec![vec![0u8; KiB!(1)]; 1];
        let mut gbl_ops = FakeGblOps::new(&storage);

        // Uploads 2k of data
        let mut data: &[u8] = &vec![0x55u8; KiB!(2)];
        let handler = &mut |out: &mut [u8]| {
            let to_read = min(out.len(), data.len());
            out[..to_read].clone_from_slice(&data[..to_read]);
            data = &data[to_read..];
            Ok((to_read, data.len()))
        };
        gbl_ops.get_staged_handler = Some(handler);

        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"download:0x400");
        listener.add_transport_input(&[0xaau8; 0x400]);
        listener.add_transport_input(b"upload");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"DATA00000400",
                b"OKAY",
                b"INFOA previous download is discarded.",
                b"INFOUploading 2048 bytes...",
                b"DATA00000800",
                &vec![0x55u8; 1024],
                &vec![0x55u8; 1024],
                b"OKAY",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_upload_no_data() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(1)]; 1];
        let mut gbl_ops = FakeGblOps::new(&storage);

        let handler = &mut |_: &mut [u8]| Ok((0, 0));
        gbl_ops.get_staged_handler = Some(handler);

        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"upload");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"FAILNo data staged.",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_upload_size_overflows() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(1)]; 1];
        let mut gbl_ops = FakeGblOps::new(&storage);

        let handler = &mut |_: &mut [u8]| Ok((0, 0x80000000));
        gbl_ops.get_staged_handler = Some(handler);

        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"upload");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"FAILCannot upload more than 0x7fffffff bytes of data",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_upload_inconsistent_data_size() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(1)]; 1];
        let mut gbl_ops = FakeGblOps::new(&storage);

        // Returns a `remains` that always equals 10.
        let handler = &mut |_: &mut [u8]| Ok((1, 10));
        gbl_ops.get_staged_handler = Some(handler);

        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"upload");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"INFOUploading 10 bytes...",
                b"DATA0000000a",
                b"\0\0\0\0\0\0\0\0\0\0",
                b"FAILStaged data size changed when uploading",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_fastboot_flashing_lock_unlock() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(1)]; 1];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"flashing lock");
        listener.add_transport_input(b"flashing unlock");
        listener.add_transport_input(b"flashing lock_critical");
        listener.add_transport_input(b"flashing unlock_critical");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"OKAY",
                b"OKAY",
                b"OKAY",
                b"OKAY",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        assert_eq!(
            gbl_ops.write_lock_state_traces,
            vec![
                (LockType::Device, LockState::Locked),
                (LockType::Device, LockState::Unlocked),
                (LockType::Critical, LockState::Locked),
                (LockType::Critical, LockState::Unlocked),
            ]
        )
    }

    #[test]
    fn test_vendor_erase() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(1)]; 1];
        let mut vendor_erase_handler = |part: &str| {
            // Skips erasing if partition is "boot_a".
            Ok(match part {
                "boot_a" => FastbootEraseAction::Noop,
                _ => FastbootEraseAction::EraseAsPhysicalPartition,
            })
        };
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.vendor_erase_handler = Some(&mut vendor_erase_handler);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        // Succeeds due to `vendor_erase_handler` instructing it to skip, even if there is no such
        // partition.
        listener.add_transport_input(b"erase:boot_a");
        // Fails as usual.
        listener.add_transport_input(b"erase:boot_b");
        // Fails. vendor_erase only called when erasing full partition with no block/subrange
        // selection
        listener.add_transport_input(b"erase:boot_a/0");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));
        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"OKAY",
                b"FAILNotFound",
                b"FAILInvalid block ID",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        )
    }

    #[test]
    fn test_fastboot_command_exec_custom_impl() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(1)]; 1];
        let mut download_trace = vec![];
        let mut handler = |cmd: Vec<String>, download: &mut [u8], download_used: usize| {
            assert_eq!(download.len(), download_used);
            download_trace.push(download.to_vec());
            match cmd.join(":").as_str() {
                "flash custom_test_okay" => Ok(CommandExecType::CustomImpl),
                "flash custom_test_fail" => Ok(CommandExecType::CustomImpl),
                _ => Ok(CommandExecType::DefaultImpl),
            }
        };
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.command_exec_send_messages = vec![
            vec![
                SenderMessage::Info("okay info 1".to_string()),
                SenderMessage::Info("okay info 2".to_string()),
                SenderMessage::Okay("ok".to_string()),
            ],
            vec![
                SenderMessage::Info("fail info".to_string()),
                SenderMessage::Fail("fail".to_string()),
            ],
        ]
        .into();
        gbl_ops.fastboot_command_exec_handler = Some(&mut handler);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"flash custom_test_okay");
        listener.add_transport_input(b"flash custom_test_fail");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<2>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));
        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"INFOokay info 1",
                b"INFOokay info 2",
                b"OKAYok",
                b"INFOfail info",
                b"FAILfail",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        assert_eq!(download_trace, vec![vec![], vec![]]);
    }

    #[test]
    fn test_fastboot_command_exec_custom_no_reply() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_a", [0u8; KiB!(4)]);
        let buffers = vec![vec![0u8; KiB!(1)]; 1];
        let mut download_trace = vec![];
        let mut handler = |cmd: Vec<String>, download: &mut [u8], download_used: usize| {
            assert_eq!(download.len(), download_used);
            download_trace.push(download.to_vec());
            match cmd.join(":").as_str() {
                "flash custom_test_no_reply" => Ok(CommandExecType::CustomImpl),
                _ => Ok(CommandExecType::DefaultImpl),
            }
        };
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.fastboot_command_exec_handler = Some(&mut handler);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"flash custom_test_no_reply");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<2>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));
        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[b"OKAY", b"INFOSyncing storage...", b"OKAY",]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        assert_eq!(download_trace, vec![vec![]]);
    }

    #[test]
    fn test_fastboot_command_exec() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_a", [0u8; KiB!(4)]);
        const BUFFER_SIZE: usize = KiB!(2);
        let buffers = vec![vec![0u8; BUFFER_SIZE]; 1];
        let mut download_trace: Vec<(Vec<u8>, usize)> = vec![];
        let mut handler = |cmd: Vec<String>, download: &mut [u8], download_used: usize| {
            download_trace.push((download.to_vec(), download_used));
            match cmd.join(":").as_str() {
                "flash not_allowed" => Ok(CommandExecType::Prohibited),
                _ => Ok(CommandExecType::DefaultImpl),
            }
        };
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.fastboot_command_exec_handler = Some(&mut handler);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        let download_data = &[0xaau8; 0x400];
        listener.add_transport_input(format!("download:{:#x}", download_data.len()).as_bytes());
        listener.add_transport_input(download_data);
        listener.add_transport_input(b"flash:boot_a");
        // Fails due to permission.
        listener.add_transport_input(b"flash not_allowed");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<2>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));
        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"DATA00000400",
                b"OKAY",
                b"OKAY",
                b"FAILCommand not allowed.",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        let mut expected_download_data_buffer = vec![0u8; BUFFER_SIZE];
        expected_download_data_buffer[..download_data.len()].copy_from_slice(download_data);
        assert_eq!(
            download_trace,
            vec![(expected_download_data_buffer.to_vec(), download_data.len()), (vec![], 0)]
        );
    }

    #[test]
    fn test_gbl_oem_dump_partition_info() {
        let mut storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(1)]; 1];
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_2.bin"));
        storage.add_raw_device(c"raw_0", [0xaau8; KiB!(4)]);
        storage.add_raw_device(c"raw_1", [0x55u8; KiB!(8)]);
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"oem gbl-partition-info");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<2>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"INFO<block ID>: <partition>, <range>, <size>",
                b"INFO0: boot_a, [0x4400, 0x6400), 0x2000",
                b"INFO0: boot_b, [0x6400, 0x9400), 0x3000",
                b"INFO1: vendor_boot_a, [0x4400, 0x5400), 0x1000",
                b"INFO1: vendor_boot_b, [0x5400, 0x6c00), 0x1800",
                b"INFO2: raw_0, [0x0, 0x1000), 0x1000",
                b"INFO3: raw_1, [0x0, 0x2000), 0x2000",
                b"OKAY",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_gbl_getvar_slotted_partition() {
        let mut storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(1)]; 1];
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_1.bin"));
        storage.add_gpt_device(include_bytes!("../../../libstorage/test/gpt_test_2.bin"));
        storage.add_raw_device(c"raw_a", [0xaau8; KiB!(4)]);
        storage.add_raw_device(c"raw_b", [0x55u8; KiB!(4)]);
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.current_slot = Some(Ok(1));
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"getvar:partition-size:boot_ab");
        listener.add_transport_input(b"getvar:partition-size:raw_ab");
        listener.add_transport_input(b"getvar:partition-size:boot");
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<2>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"FAILboot_b and boot_a has different partition sizes",
                b"OKAY0x1000",
                b"OKAY0x3000",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    fn gbl_getvar_unlocked_test_helper(command: &str, status: AvbDeviceStatus, expected_str: &str) {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(1)]; 1];
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.avb_device_status_error = None;
        gbl_ops.avb_device_status = status;
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(command.as_bytes());
        listener.add_transport_input(b"continue");
        block_on(run_gbl_fastboot_stack::<2>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                expected_str.as_bytes(),
                b"INFOSyncing storage...",
                b"OKAY"
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
    }

    #[test]
    fn test_gbl_getvar_unlocked_device_unlocked() {
        gbl_getvar_unlocked_test_helper(
            "getvar:unlocked",
            AvbDeviceStatus { is_unlocked: true, ..Default::default() },
            "OKAYyes",
        );
    }

    #[test]
    fn test_gbl_getvar_unlocked_device_locked() {
        gbl_getvar_unlocked_test_helper(
            "getvar:unlocked",
            AvbDeviceStatus { is_unlocked: false, ..Default::default() },
            "OKAYno",
        );
    }

    #[test]
    fn test_gbl_getvar_unlocked_critical_unlocked() {
        gbl_getvar_unlocked_test_helper(
            "getvar:unlocked-critical",
            AvbDeviceStatus { is_unlocked_critical: true, ..Default::default() },
            "OKAYyes",
        );
    }

    #[test]
    fn test_gbl_getvar_unlocked_critical_locked() {
        gbl_getvar_unlocked_test_helper(
            "getvar:unlocked-critical",
            AvbDeviceStatus { is_unlocked_critical: false, ..Default::default() },
            "OKAYno",
        );
    }

    #[test]
    fn test_gbl_oem_add_cmdline_bootconfig() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(1)]; 1];
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.avb_device_status.is_unlocked = true;
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"oem gbl-add-cmdline");
        listener.add_transport_input(b"oem gbl-add-bootconfig");
        listener.add_transport_input(b"oem gbl-add-cmdline arg0=val0");
        listener.add_transport_input(b"oem gbl-add-bootconfig arg1=val2");
        listener.add_transport_input(b"oem gbl-add-cmdline arg2=val2");
        listener.add_transport_input(b"oem gbl-add-bootconfig arg3=val3");
        let download_data = b"some test data";
        listener.add_transport_input(format!("download:{:#x}", download_data.len()).as_bytes());
        listener.add_transport_input(download_data);
        listener.add_transport_input(b"oem gbl-add-staged-data"); // Failed. Missing tag.
        listener.add_transport_input(b"oem gbl-add-staged-data test");
        listener.add_transport_input(b"continue");
        let mut general = vec![0u8; 1024];
        block_on(run_gbl_fastboot_stack::<2>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            GblFbData { boot_buffer: (&mut general[..]).into(), ..Default::default() },
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"FAILMissing cmdline arg",
                b"FAILMissing bootconfig arg",
                b"OKAY",
                b"OKAY",
                b"OKAY",
                b"OKAY",
                b"DATA0000000e",
                b"OKAY",
                b"FAILMissing tag",
                b"OKAY",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        let container = BootItemContainer::new(&mut general[..]);
        assert_eq!(
            container
                .iter()
                .map(|(i, v)| (i, String::from_utf8(v.to_vec()).unwrap()))
                .collect::<Vec<_>>(),
            vec![
                (BootItem::Cmdline, "arg0=val0".into()),
                (BootItem::Bootconfig, "arg1=val2".into()),
                (BootItem::Cmdline, "arg2=val2".into()),
                (BootItem::Bootconfig, "arg3=val3".into()),
                (BootItem::Bootconfig, "gbl.blob.test=c29tZSB0ZXN0IGRhdGE=".into()),
            ]
        );
    }

    #[test]
    fn test_gbl_oem_add_cmdline_bootconfig_fail_whiled_locked() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(1)]; 1];
        let mut gbl_ops = FakeGblOps::new(&storage);
        gbl_ops.avb_device_status.is_unlocked = false;
        let listener: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener], &listener);
        listener.add_transport_input(b"oem gbl-add-cmdline arg0=val0");
        listener.add_transport_input(b"oem gbl-add-bootconfig arg1=val2");
        let download_data = b"some test data";
        listener.add_transport_input(format!("download:{:#x}", download_data.len()).as_bytes());
        listener.add_transport_input(download_data);
        listener.add_transport_input(b"oem gbl-add-staged-data test");
        listener.add_transport_input(b"continue");
        let mut general = vec![0u8; 1024];
        block_on(run_gbl_fastboot_stack::<2>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            GblFbData { boot_buffer: (&mut general[..]).into(), ..Default::default() },
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"FAILDevice is not unlocked",
                b"FAILDevice is not unlocked",
                b"DATA0000000e",
                b"OKAY",
                b"FAILDevice is not unlocked",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        assert_eq!(BootItemContainer::new(&mut general[..]).iter().next(), None);
    }

    #[test]
    fn test_gbl_fastboot_disconnection() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"raw", [0u8; KiB!(2)]);
        let buffers = vec![vec![0u8; KiB!(2)]; 1];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let mut transport_out_err = |v: &[u8]| -> Result<(), Error> {
            // The response is from "getvar:partition-size:raw"
            (v == b"OKAY0x800").then_some(Err(Error::Disconnected)).unwrap_or(Ok(()))?;
            // Data uploaded by '"fetch:raw:0:0x800"
            (v[0] == 0x55).then_some(Err(Error::Disconnected)).unwrap_or(Ok(()))?;
            Ok(())
        };
        let listener: SharedTestListener = Default::default();
        listener.lock().transport_out_err = Some(&mut transport_out_err);
        let (transports, tcp) = (&mut [&listener], &listener);

        // Download interrupted
        listener.add_transport_input(format!("download:{:#x}", KiB!(2)).as_bytes());
        listener.add_transport_input(&[0x55u8; KiB!(1)]);
        listener.add_transport_err(Error::Disconnected);
        // Previous donwload failure shouldn't affect future commands or download.
        listener.add_transport_input(format!("download:{:#x}", KiB!(2)).as_bytes());
        listener.add_transport_input(&[0x55u8; KiB!(2)]);
        listener.add_transport_input(b"flash:raw");
        // Transport send error.
        listener.add_transport_input(b"getvar:partition-size:raw");
        // Transpor send error during data upload.
        listener.add_transport_input(b"fetch:raw:0:0x800");
        // Previous send error shouldn't affect future commands.
        listener.add_transport_input(b"continue");
        let mut general = vec![0u8; 1024];
        block_on(run_gbl_fastboot_stack::<2>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            GblFbData { boot_buffer: (&mut general[..]).into(), ..Default::default() },
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"DATA00000800",
                b"DATA00000800",
                b"OKAY",
                b"OKAY",
                b"INFOUploading 2048 bytes...",
                b"DATA00000800",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );
        // Verifies flashed image on raw.
        assert_eq!(storage[0].partition_io(None).unwrap().dev().io().storage, [0x55u8; KiB!(2)]);
    }

    #[test]
    fn test_run_gbl_fastboot_multiple_transports() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let listener1: SharedTestListener = Default::default();
        let listener2: SharedTestListener = Default::default();
        let (transports, tcp) = (&mut [&listener1, &listener2], &listener);

        listener1.add_transport_input(b"getvar:version-bootloader");
        listener2.add_transport_input(b"oem test-oem");
        listener.add_tcp_input(b"FB01");
        listener.add_tcp_length_prefixed_input(b"getvar:max-download-size");
        listener.add_tcp_length_prefixed_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener1.transport_out_queue(),
            make_expected_transport_out(&[
                format!("OKAY{}", expected_version_bootloader()).as_bytes()
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        assert_eq!(
            listener2.transport_out_queue(),
            make_expected_transport_out(&[
                format!("INFO{}", FakeGblOps::GBL_OEM_CMD_INFO_MSG).as_bytes(),
                b"OKAY",
            ]),
            "\nActual Transport output:\n{}",
            listener.dump_transport_out_queue()
        );

        assert_eq!(
            listener.tcp_out_queue(),
            make_expected_tcp_out(&[b"OKAY0x20000", b"INFOSyncing storage...", b"OKAY"]),
            "\nActual TCP output:\n{}",
            listener.dump_tcp_out_queue()
        );
    }

    #[test]
    fn test_run_gbl_fastboot_tcp_no_transports() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (transports, tcp): (&mut [&SharedTestListener], _) = (&mut [], &listener);

        listener.add_tcp_input(b"FB01");
        listener.add_tcp_length_prefixed_input(b"getvar:max-download-size");
        listener.add_tcp_length_prefixed_input(b"continue");
        block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            Some(tcp),
            Default::default(),
        ));

        assert_eq!(
            listener.tcp_out_queue(),
            make_expected_tcp_out(&[b"OKAY0x20000", b"INFOSyncing storage...", b"OKAY"]),
            "\nActual TCP output:\n{}",
            listener.dump_tcp_out_queue()
        );
    }

    #[test]
    fn test_run_gbl_fastboot_no_transports() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(128)]; 2];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let transports: &mut [&SharedTestListener] = &mut [];
        let res = block_on(run_gbl_fastboot_stack::<3>(
            &mut gbl_ops,
            buffers,
            transports,
            None::<&SharedTestListener>,
            Default::default(),
        ));

        assert_eq!(res, Default::default());
    }

    #[test]
    fn test_gbl_fastboot_download_crc_check() {
        let storage = FakeGblOpsStorage::default();
        let buffers = vec![vec![0u8; KiB!(2)]; 1];
        let mut gbl_ops = FakeGblOps::new(&storage);
        let listener: SharedTestListener = Default::default();
        let (generic, tcp) = (&mut [&listener], &listener);

        // By default no crc check
        listener.add_transport_input(format!("download:{:#x}", KiB!(2)).as_bytes());
        listener.add_transport_input(&[0x55u8; KiB!(2)]);
        // Mismatched crc value
        listener.add_transport_input(b"oem gbl-set-download-crc 0");
        listener.add_transport_input(format!("download:{:#x}", KiB!(2)).as_bytes());
        listener.add_transport_input(&[0x55u8; KiB!(2)]);
        // "oem gbl-set-download-crc" takes effect only once.
        listener.add_transport_input(format!("download:{:#x}", KiB!(2)).as_bytes());
        listener.add_transport_input(&[0x55u8; KiB!(2)]);
        // Correct crc value.
        listener.add_transport_input(b"oem gbl-set-download-crc b47c63c1");
        listener.add_transport_input(format!("download:{:#x}", KiB!(2)).as_bytes());
        listener.add_transport_input(&[0x55u8; KiB!(2)]);
        // Canceled crc check.
        listener.add_transport_input(b"oem gbl-set-download-crc 0");
        listener.add_transport_input(b"oem gbl-unset-download-crc");
        listener.add_transport_input(format!("download:{:#x}", KiB!(2)).as_bytes());
        listener.add_transport_input(&[0x55u8; KiB!(2)]);
        listener.add_transport_input(b"continue");

        let mut general = vec![0u8; 1024];
        block_on(run_gbl_fastboot_stack::<2>(
            &mut gbl_ops,
            buffers,
            generic,
            Some(tcp),
            GblFbData { boot_buffer: (&mut general[..]).into(), ..Default::default() },
        ));

        assert_eq!(
            listener.transport_out_queue(),
            make_expected_transport_out(&[
                b"DATA00000800",
                b"OKAY",
                b"OKAY",
                b"DATA00000800",
                b"FAILCRC check failed. expect: 0x0, actual: 0xb47c63c1",
                b"DATA00000800",
                b"OKAY",
                b"OKAY",
                b"DATA00000800",
                b"OKAY",
                b"OKAY",
                b"OKAY",
                b"DATA00000800",
                b"OKAY",
                b"INFOSyncing storage...",
                b"OKAY",
            ]),
            "\nActual USB output:\n{}",
            listener.dump_transport_out_queue()
        );
    }
}
