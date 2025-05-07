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

//! Android boot support.

use crate::{
    constants::{FDT_ALIGNMENT, KERNEL_ALIGNMENT},
    device_tree::{
        DeviceTreeComponentSource, DeviceTreeComponentType, DeviceTreeComponentsRegistry,
    },
    fastboot::{
        run_gbl_fastboot, run_gbl_fastboot_stack, BufferPool, GblFastbootResult, GblTcpStream,
        GblUsbTransport, LoadedImageInfo, PinFutContainer, Shared,
    },
    gbl_println,
    ops::RebootReason,
    GblOps, Result,
};
use bootparams::commandline::CommandlineBuilder;
use core::{array::from_fn, ffi::CStr};
use dttable::DtTableImage;
use fastboot::local_session::LocalSession;
use fdt::{Fdt, FdtHeader};
use gbl_async::block_on;
use liberror::Error;
use libutils::{aligned_offset, aligned_subslice};
use misc::{AndroidBootMode, BootloaderMessage};
use safemath::SafeNum;

mod avf;
use avf::{pkvm_describe_pvmfw_resvmem, pvmfw_place_in_memory};

mod vboot;
use vboot::{avb_verify_slot, PartitionsToVerify};

pub(crate) mod load;
use load::split_chunks;
pub use load::{android_load_verify, get_kernel};

/// Device tree bootargs property to store kernel command line.
pub const BOOTARGS_PROP: &CStr = c"bootargs";

/// A helper to convert a bytes slice containing a null-terminated string to `str`
fn cstr_bytes_to_str(data: &[u8]) -> core::result::Result<&str, Error> {
    Ok(CStr::from_bytes_until_nul(data)?.to_str()?)
}

/// Loads Android images from the given slot on disk and fixes up bootconfig, commandline, and FDT.
///
/// On success, returns a tuple of (ramdisk, fdt, kernel, unused buffer).
pub fn android_load_verify_fixup<'a, 'b, 'c>(
    ops: &mut impl GblOps<'b, 'c>,
    slot: u8,
    is_recovery: bool,
    load: &'a mut [u8],
) -> Result<(&'a mut [u8], &'a mut [u8], &'a mut [u8], &'a mut [u8])> {
    let load_addr = load.as_ptr() as usize;
    let images = android_load_verify(ops, slot, is_recovery, load)?;

    let mut components = DeviceTreeComponentsRegistry::new();
    let fdt_load = &mut images.unused[..];
    // TODO(b/353272981): Remove get_custom_device_tree
    let (fdt_load, base, overlays) = match ops.get_custom_device_tree() {
        Some(v) => (fdt_load, v, &[][..]),
        _ => {
            let mut remains = match images.dtbo.len() > 0 {
                // TODO(b/384964561, b/374336105): Investigate if we can avoid additional copy.
                true => {
                    gbl_println!(ops, "Handling overlays from dtbo");
                    components.append_from_dttable(
                        DeviceTreeComponentSource::Dtbo,
                        DeviceTreeComponentType::Overlay,
                        &DtTableImage::from_bytes(images.dtbo)?,
                        fdt_load,
                    )?
                }
                _ => fdt_load,
            };

            if images.dtb.len() > 0 {
                gbl_println!(ops, "Handling device tree from boot/vendor_boot");
                remains = if FdtHeader::from_bytes_ref(images.dtb).is_ok() {
                    gbl_println!(ops, "Device tree found in boot/vendor_boot");
                    components.append(
                        ops,
                        DeviceTreeComponentSource::Boot,
                        DeviceTreeComponentType::DeviceTree,
                        images.dtb,
                        remains,
                    )?
                } else if let Ok(table) = DtTableImage::from_bytes(images.dtb) {
                    gbl_println!(
                        ops,
                        "Dttable with {} entries found in boot/vendor_boot",
                        table.entries_count()
                    );
                    components.append_from_dttable(
                        DeviceTreeComponentSource::Boot,
                        DeviceTreeComponentType::DeviceTree,
                        &table,
                        remains,
                    )?
                } else {
                    return Err(Error::Other(Some(
                        "Invalid or unrecognized device tree format in boot/vendor_boot",
                    ))
                    .into());
                }
            }

            if images.dtb_part.len() > 0 {
                gbl_println!(ops, "Handling device trees from dtb");
                let dttable = DtTableImage::from_bytes(images.dtb_part)?;
                remains = components.append_from_dttable(
                    DeviceTreeComponentSource::Dtb,
                    DeviceTreeComponentType::DeviceTree,
                    &dttable,
                    remains,
                )?;
            }

            gbl_println!(ops, "Selecting device tree components");
            ops.select_device_trees(&mut components)?;
            let (base, overlays) = components.selected()?;
            (remains, base, overlays)
        }
    };
    let fdt_load = aligned_subslice(fdt_load, FDT_ALIGNMENT)?;
    let mut fdt = Fdt::new_from_init(&mut fdt_load[..], base)?;

    // Adds ramdisk range to FDT
    let ramdisk_addr: u64 = (images.ramdisk.as_ptr() as usize).try_into().map_err(Error::from)?;
    let ramdisk_end: u64 = ramdisk_addr + u64::try_from(images.ramdisk.len()).unwrap();
    fdt.set_property("chosen", c"linux,initrd-start", &ramdisk_addr.to_be_bytes())?;
    fdt.set_property("chosen", c"linux,initrd-end", &ramdisk_end.to_be_bytes())?;
    gbl_println!(ops, "linux,initrd-start: {:#x}", ramdisk_addr);
    gbl_println!(ops, "linux,initrd-end: {:#x}", ramdisk_end);

    // Updates the FDT commandline.
    let device_tree_commandline_length = match fdt.get_property("chosen", BOOTARGS_PROP) {
        Ok(val) => CStr::from_bytes_until_nul(val).map_err(Error::from)?.to_bytes().len(),
        Err(_) => 0,
    };

    // Reserves 1024 bytes for separators and fixup.
    let final_commandline_len = device_tree_commandline_length
        + images.boot_cmdline.len()
        + images.vendor_cmdline.len()
        + 1024;
    let final_commandline_buffer =
        fdt.set_property_placeholder("chosen", BOOTARGS_PROP, final_commandline_len)?;
    let mut commandline_builder =
        CommandlineBuilder::new_from_prefix(&mut final_commandline_buffer[..])?;
    commandline_builder.add(images.boot_cmdline)?;
    commandline_builder.add(images.vendor_cmdline)?;

    // TODO(b/353272981): Handle buffer too small
    commandline_builder.add_with(|current, out| {
        // TODO(b/353272981): Verify provided command line and fail here.
        Ok(ops.fixup_os_commandline(current, out)?.map(|fixup| fixup.len()).unwrap_or(0))
    })?;
    gbl_println!(ops, "final cmdline: \"{}\"", commandline_builder.as_str());

    gbl_println!(ops, "Applying {} overlays", overlays.len());
    fdt.multioverlay_apply(overlays)?;
    gbl_println!(ops, "Overlays applied");
    // `DeviceTreeComponentsRegistry` internally uses ArrayVec which causes it to have a default
    // life time equal to the scope it lives in. This is unnecessarily strict and prevents us from
    // accessing `load` buffer.
    drop(components);

    // Place pvmfw binary into reserved memory
    if images.pvmfw.len() > 0 {
        let pvmfw_image_buf = pvmfw_place_in_memory(ops, images.pvmfw)?;
        pkvm_describe_pvmfw_resvmem(&mut fdt, &pvmfw_image_buf)?;
        gbl_println!(ops, "AVF: init success");
    }

    // Make sure we provide an actual device tree size, so FW can calculate amount of space
    // available for fixup.
    fdt.shrink_to_fit()?;
    // TODO(b/353272981): Make a copy of current device tree and verify provided fixup.
    // TODO(b/353272981): Handle buffer too small
    ops.fixup_device_tree(fdt.as_mut())?;
    fdt.shrink_to_fit()?;

    // Moves the kernel forward to reserve as much space as possible. This is in case there is not
    // enough memory after `load`, i.e. the memory after it is not mapped or is reserved.
    let ramdisk_off = usize::try_from(ramdisk_addr).unwrap() - load_addr;
    let fdt_len = fdt.header_ref()?.actual_size();
    let fdt_off = fdt_load.as_ptr() as usize - load_addr;
    let kernel_off = images.kernel.as_ptr() as usize - load_addr;
    let kernel_len = images.kernel.len();
    let mut kernel_new = (SafeNum::from(fdt_off) + fdt_len).try_into().map_err(Error::from)?;
    kernel_new += aligned_offset(&mut load[kernel_new..], KERNEL_ALIGNMENT)?;
    load.copy_within(kernel_off..kernel_off + kernel_len, kernel_new);
    let ([_, ramdisk, fdt, kernel], unused) =
        split_chunks(load, &[ramdisk_off, fdt_off - ramdisk_off, kernel_new - fdt_off, kernel_len]);
    let ramdisk = &mut ramdisk[..usize::try_from(ramdisk_end - ramdisk_addr).unwrap()];
    Ok((ramdisk, fdt, kernel, unused))
}

/// Gets the target slot to boot.
///
/// * If GBL is slotless (`GblOps::get_current_slot()` returns `Error::Unsupported`), the API
///   behaves the same as `GblOps::get_next_slot()`.
/// * If GBL is slotted, the API behaves the same as `GblOps::get_current_slot()` and
///   `mark_boot_attempt` is ignored.
/// * Default to A slot if slotting backend is not implemented on the platform.
pub(crate) fn get_boot_slot<'a, 'b, 'c>(
    ops: &mut impl GblOps<'a, 'b>,
    mark_boot_attempt: bool,
) -> Result<char> {
    let slot = match ops.get_current_slot() {
        // Slotless bootloader
        Err(Error::Unsupported) => {
            gbl_println!(ops, "GBL is Slotless.");
            ops.get_next_slot(mark_boot_attempt)
        }
        v => v,
    };
    match slot {
        Ok(slot) => Ok(slot.suffix.0),
        Err(Error::Unsupported) | Err(Error::NotFound) => {
            // Default to slot A if slotting is not supported.
            // Slotless partition name is currently not supported. Revisit if this causes problems.
            gbl_println!(ops, "Slotting is not supported. Choose A slot by default");
            Ok('a')
        }
        Err(e) => {
            gbl_println!(ops, "Failed to get boot slot: {e}");
            Err(e.into())
        }
    }
}

/// Provides methods to run GBL fastboot.
pub struct GblFastbootEntry<'d, G> {
    pub(crate) ops: &'d mut G,
    pub(crate) load: &'d mut [u8],
    pub(crate) result: &'d mut GblFastbootResult,
}

impl<'a, 'd, 'e, G> GblFastbootEntry<'d, G>
where
    G: GblOps<'a, 'e>,
{
    /// Runs GBL fastboot with the given buffer pool, tasks container, and usb/tcp/local transport
    /// channels.
    ///
    /// # Args
    ///
    /// * `buffer_pool`: An implementation of `BufferPool` wrapped in `Shared` for allocating
    ///    download buffers.
    /// * `tasks`: An implementation of `PinFutContainer` used as task container for GBL fastboot to
    // /   schedule dynamically spawned async tasks.
    /// * `local`: An implementation of `LocalSession` which exchanges fastboot packet from platform
    ///   specific channels i.e. UX.
    /// * `usb`: An implementation of `GblUsbTransport` that represents USB channel.
    /// * `tcp`: An implementation of `GblTcpStream` that represents TCP channel.
    pub async fn run<'b: 'c, 'c>(
        self,
        buffer_pool: &'b Shared<impl BufferPool>,
        tasks: impl PinFutContainer<'c> + 'c,
        local: Option<impl LocalSession>,
        usb: Option<impl GblUsbTransport>,
        tcp: Option<impl GblTcpStream>,
    ) where
        'a: 'c,
        'd: 'c,
    {
        *self.result =
            run_gbl_fastboot(self.ops, buffer_pool, tasks, local, usb, tcp, self.load).await;
    }

    /// Runs fastboot with N pre-allocated async worker tasks.
    ///
    /// Comparing  to `Self::run()`, this API   simplifies the input by handling the implementation of
    /// `BufferPool` and `PinFutContainer` internally . However it only supports up to N parallel
    /// tasks where N is determined at build time. The download buffer will be split into N chunks
    /// evenly.
    ///
    /// The choice of N depends on the level of parallelism the platform can support. For platform
    /// with `n` storage devices that can independently perform non-blocking IO, it will required
    /// `N = n + 1` in order to achieve parallel flashing to all storages plus a parallel download.
    /// However, it is common for partitions that need to be flashed to be on the same block device
    /// so flashing of them becomes sequential, in which case N can be smaller. Caller should take
    /// into consideration usage pattern for determining N. If platform only has one physical disk
    /// or does not expect disks to be parallelizable, a common choice is N=2 which allows
    /// downloading and flashing to be performed in parallel.
    pub fn run_n<const N: usize>(
        self,
        download: &mut [u8],
        local: Option<impl LocalSession>,
        usb: Option<impl GblUsbTransport>,
        tcp: Option<impl GblTcpStream>,
    ) {
        if N < 1 {
            return self.run_n::<1>(download, local, usb, tcp);
        }
        // Splits into N download buffers.
        let mut arr: [_; N] = from_fn(|_| Default::default());
        for (i, v) in download.chunks_exact_mut(download.len() / N).enumerate() {
            arr[i] = v;
        }
        let bufs = &mut arr[..];
        *self.result =
            block_on(run_gbl_fastboot_stack::<N>(self.ops, bufs, local, usb, tcp, self.load));
    }
}

/// Runs full Android bootloader bootflow before kernel handoff.
///
/// The API performs slot selection, handles boot mode, fastboot and loads and verifies Android from
/// disk.
///
/// # Args:
///
/// * `ops`: An implementation of `GblOps`.
/// * `load`: Buffer for loading various Android images.
/// * `run_fastboot`: A closure for running GBL fastboot. The closure is passed a
///   `GblFastbootEntry` type which provides methods for running GBL fastboot. The caller is
///   responsible for preparing the required inputs and calling the method in the closure. See
///   `GblFastbootEntry` for more details.
///
/// On success, returns a tuple of slices corresponding to `(ramdisk, FDT, kernel, unused)`
pub fn android_main<'a, 'b, 'c, G: GblOps<'a, 'b>>(
    ops: &mut G,
    load: &'c mut [u8],
    run_fastboot: impl FnOnce(GblFastbootEntry<'_, G>),
) -> Result<(&'c mut [u8], &'c mut [u8], &'c mut [u8], &'c mut [u8])> {
    let (bcb_buffer, _) = load
        .split_at_mut_checked(BootloaderMessage::SIZE_BYTES)
        .ok_or(Error::BufferTooSmall(Some(BootloaderMessage::SIZE_BYTES)))
        .inspect_err(|e| gbl_println!(ops, "Buffer too small for reading misc. {e}"))?;
    ops.read_from_partition_sync("misc", 0, bcb_buffer)
        .inspect_err(|e| gbl_println!(ops, "Failed to read misc partition: {e}"))?;
    let bcb = BootloaderMessage::from_bytes_ref(bcb_buffer)
        .inspect_err(|e| gbl_println!(ops, "Failed to parse bootloader messgae: {e}"))?;
    let boot_mode = bcb
        .boot_mode()
        .inspect_err(|e| gbl_println!(ops, "Failed to parse BCB boot mode ({e}). Ignored"))
        .unwrap_or(AndroidBootMode::Normal);
    gbl_println!(ops, "Boot mode from BCB: {}", boot_mode);

    if matches!(boot_mode, AndroidBootMode::BootloaderBootOnce) {
        let mut zeroed_command = [0u8; misc::COMMAND_FIELD_SIZE];
        ops.write_to_partition_sync(
            "misc",
            misc::COMMAND_FIELD_OFFSET.try_into().unwrap(),
            &mut zeroed_command,
        )?;
    }

    // Checks platform reboot reason.
    let reboot_reason = ops
        .get_reboot_reason()
        .inspect_err(|e| {
            gbl_println!(ops, "Failed to get reboot reason from platform: {e}. Ignored.")
        })
        .unwrap_or(RebootReason::Normal);
    gbl_println!(ops, "Reboot reason from platform: {reboot_reason:?}");

    // Checks and enters fastboot.
    let result = &mut Default::default();
    if matches!(reboot_reason, RebootReason::Bootloader)
        || matches!(boot_mode, AndroidBootMode::BootloaderBootOnce)
        || ops
            .should_stop_in_fastboot()
            .inspect_err(|e| {
                gbl_println!(ops, "Warning: error while checking fastboot trigger ({:?})", e);
                gbl_println!(ops, "Ignoring error and continuing with normal boot");
            })
            .unwrap_or(false)
    {
        gbl_println!(ops, "Entering fastboot mode...");
        run_fastboot(GblFastbootEntry { ops, load: &mut load[..], result });
        gbl_println!(ops, "Leaving fastboot mode...");
    }

    // Checks if "fastboot boot" has loaded an android image.
    match &result.loaded_image_info {
        Some(LoadedImageInfo::Android { .. }) => {
            gbl_println!(ops, "Booting from \"fastboot boot\"");
            return Ok(result.split_loaded_android(load).unwrap());
        }
        _ => {}
    }

    // Checks whether fastboot has set a different active slot. Reboot if it does.
    let slot_suffix = get_boot_slot(ops, true)?;
    if result.last_set_active_slot.unwrap_or(slot_suffix) != slot_suffix {
        gbl_println!(ops, "Active slot changed by \"fastboot set_active\". Reset..");
        ops.reboot();
        return Err(Error::UnexpectedReturn.into());
    }

    // Currently we assume slot suffix only takes value within 'a' to 'z'. Revisit if this
    // is not the case.
    //
    // It's a little awkward to convert suffix char to integer which will then be converted
    // back to char by the API. Consider passing in the char bytes directly.
    let slot_idx = (u64::from(slot_suffix) - u64::from('a')).try_into().unwrap();

    let is_recovery = matches!(reboot_reason, RebootReason::Recovery)
        || matches!(boot_mode, AndroidBootMode::Recovery);
    android_load_verify_fixup(ops, slot_idx, is_recovery, load)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        fastboot::test::{make_expected_usb_out, SharedTestListener, TestLocalSession},
        gbl_avb::state::{BootStateColor, KeyValidationStatus},
        ops::test::{slot, FakeGblOps, FakeGblOpsStorage},
        tests::AlignedBuffer,
    };
    use bootparams::bootconfig::{BootConfigBuilder, BOOTCONFIG_TRAILER_SIZE};
    use libbuild_number::BUILD_NUMBER;
    use std::{
        ascii::escape_default, collections::HashMap, ffi::CString, fs, path::Path, string::String,
    };

    const TEST_ROLLBACK_INDEX_LOCATION: usize = 1;

    // The vendor bootconfig in the generated vendor boot image.
    // See libgbl/testdata/gen_test_data.py for test data generation.
    pub(crate) const TEST_VENDOR_BOOTCONFIG: &str =
        "androidboot.config_1=val_1\x0aandroidboot.config_2=val_2\x0a";

    /// Digest of public key used to execute AVB.
    pub(crate) const TEST_PUBLIC_KEY_DIGEST: &str =
        "7ec02ee1be696366f3fa91240a8ec68125c4145d698f597aa2b3464b59ca7fc3";

    // Test data path
    const TEST_DATA_PATH: &str = "external/gbl/libgbl/testdata/android";

    /// Reads a data file under libgbl/testdata/
    pub(crate) fn read_test_data(file: impl AsRef<str>) -> Vec<u8> {
        println!("reading file: {}", file.as_ref());
        fs::read(Path::new(format!("{TEST_DATA_PATH}/{}", file.as_ref()).as_str())).unwrap()
    }

    /// Reads a data file as string under libgbl/testdata/
    pub(crate) fn read_test_data_as_str(file: impl AsRef<str>) -> String {
        fs::read_to_string(Path::new(format!("{TEST_DATA_PATH}/{}", file.as_ref()).as_str()))
            .unwrap()
    }

    /// Generates a readable string for a bootconfig bytes.
    pub(crate) fn dump_bootconfig(data: &[u8]) -> String {
        let s = data.iter().map(|v| escape_default(*v).to_string()).collect::<Vec<_>>().concat();
        let s = s.split("\\\\").collect::<Vec<_>>().join("\\");
        s.split("\\n").collect::<Vec<_>>().join("\n")
    }

    /// A helper for assert checking ramdisk binary and bootconfig separately.
    pub(crate) fn check_ramdisk(ramdisk: &[u8], expected_bin: &[u8], expected_bootconfig: &[u8]) {
        let (ramdisk, bootconfig) = ramdisk.split_at(expected_bin.len());
        assert_eq!(ramdisk, expected_bin);
        assert_eq!(
            bootconfig,
            expected_bootconfig,
            "\nexpect: \n{}\nactual: \n{}\n",
            dump_bootconfig(expected_bootconfig),
            dump_bootconfig(bootconfig),
        );
    }

    /// A helper for generating avb bootconfig with the given parameters.
    pub(crate) struct AvbResultBootconfigBuilder {
        vbmeta_size: usize,
        digest: String,
        boot_digest: Option<String>,
        init_boot_digest: Option<String>,
        dtb_digest: Option<String>,
        dtbo_digest: Option<String>,
        vendor_boot_digest: Option<String>,
        public_key_digest: String,
        color: BootStateColor,
        unlocked: bool,
        extra: String,
    }

    impl AvbResultBootconfigBuilder {
        pub(crate) fn new() -> Self {
            Self {
                vbmeta_size: 0,
                digest: String::new(),
                boot_digest: None,
                init_boot_digest: None,
                dtb_digest: None,
                dtbo_digest: None,
                vendor_boot_digest: None,
                public_key_digest: String::new(),
                color: BootStateColor::Green,
                unlocked: false,
                extra: String::new(),
            }
        }

        pub(crate) fn vbmeta_size(mut self, size: usize) -> Self {
            self.vbmeta_size = size;
            self
        }

        pub(crate) fn digest(mut self, digest: impl Into<String>) -> Self {
            self.digest = digest.into();
            self
        }

        pub(crate) fn partition_digest(mut self, name: &str, digest: impl Into<String>) -> Self {
            let digest = Some(digest.into());
            match name {
                "boot" => self.boot_digest = digest,
                "init_boot" => self.init_boot_digest = digest,
                "vendor_boot" => self.vendor_boot_digest = digest,
                "dtb" => self.dtb_digest = digest,
                "dtbo" => self.dtbo_digest = digest,
                _ => panic!("unknown digest name requested"),
            };
            self
        }

        pub(crate) fn public_key_digest(mut self, pk_digest: impl Into<String>) -> Self {
            self.public_key_digest = pk_digest.into();
            self
        }

        pub(crate) fn color(mut self, color: BootStateColor) -> Self {
            self.color = color;
            self
        }

        pub(crate) fn unlocked(mut self, unlocked: bool) -> Self {
            self.unlocked = unlocked;
            self
        }

        pub(crate) fn extra(mut self, extra: impl Into<String>) -> Self {
            self.extra += &extra.into();
            self
        }

        pub(crate) fn build_string(self) -> String {
            let device_state = match self.unlocked {
                true => "unlocked",
                false => "locked",
            };

            let mut boot_digests = String::new();
            for (name, maybe_digest) in [
                ("boot", &self.boot_digest),
                ("dtb", &self.dtb_digest),
                ("dtbo", &self.dtbo_digest),
                ("init_boot", &self.init_boot_digest),
                ("vendor_boot", &self.vendor_boot_digest),
            ] {
                if let Some(digest) = maybe_digest {
                    boot_digests += format!(
                        "androidboot.vbmeta.{name}.hash_alg=sha256
androidboot.vbmeta.{name}.digest={digest}\n"
                    )
                    .as_str()
                }
            }

            format!(
                "androidboot.vbmeta.device=PARTUUID=00000000-0000-0000-0000-000000000000
androidboot.vbmeta.public_key_digest={}
androidboot.vbmeta.avb_version=1.3
androidboot.vbmeta.device_state={}
androidboot.vbmeta.hash_alg=sha512
androidboot.vbmeta.size={}
androidboot.vbmeta.digest={}
androidboot.vbmeta.invalidate_on_error=yes
androidboot.veritymode=enforcing
{}androidboot.verifiedbootstate={}
{}",
                self.public_key_digest,
                device_state,
                self.vbmeta_size,
                self.digest,
                boot_digests.as_str(),
                self.color,
                self.extra
            )
        }

        pub(crate) fn build(self) -> Vec<u8> {
            make_bootconfig(self.build_string())
        }
    }

    // A helper for generating expected bootconfig.
    pub(crate) fn make_bootconfig(bootconfig: impl AsRef<str>) -> Vec<u8> {
        let bootconfig = bootconfig.as_ref();
        let mut buffer = vec![0u8; bootconfig.len() + BOOTCONFIG_TRAILER_SIZE];
        let mut res = BootConfigBuilder::new(&mut buffer).unwrap();
        res.add_with(|_, out| {
            out[..bootconfig.len()].clone_from_slice(bootconfig.as_bytes());
            Ok(bootconfig.as_bytes().len())
        })
        .unwrap();
        res.config_bytes().to_vec()
    }

    struct MakeExpectedBootconfigInclude {
        pub boot: bool,
        pub init_boot: bool,
        pub vendor_boot: bool,
        pub dtb: bool,
        pub dtbo: bool,
    }

    impl MakeExpectedBootconfigInclude {
        fn is_include_str(&self, name: &str) -> bool {
            match name {
                "boot" => self.boot,
                "init_boot" => self.init_boot,
                "vendor_boot" => self.vendor_boot,
                "dtb" => self.dtb,
                "dtbo" => self.dtbo,
                _ => false,
            }
        }
    }

    impl Default for MakeExpectedBootconfigInclude {
        fn default() -> MakeExpectedBootconfigInclude {
            MakeExpectedBootconfigInclude {
                boot: true,
                init_boot: true,
                vendor_boot: true,
                dtb: true,
                dtbo: true,
            }
        }
    }

    /// Helper for generating expected bootconfig after load and verification.
    ///
    /// # Args
    ///
    /// * `vbmeta_file``: The test file name for the target vbmeta data.
    /// * `unlocked`: True if unlocked mode.
    /// * `color`: The expected boot state color.
    /// * `slot`: The expected slot.
    /// * `vendor_config:` The expected vendor_boot config.
    /// * `include`: Additional partition digest to include in the expected bootconfig.
    fn make_expected_bootconfig(
        vbmeta_file: &str,
        unlocked: bool,
        color: BootStateColor,
        slot: char,
        vendor_config: &str,
        include: MakeExpectedBootconfigInclude,
    ) -> Vec<u8> {
        let vbmeta_file = Path::new(vbmeta_file);
        let vbmeta_digest = vbmeta_file.with_extension("digest.txt");
        let vbmeta_digest = vbmeta_digest.to_str().unwrap();
        let mut builder = AvbResultBootconfigBuilder::new()
            .vbmeta_size(read_test_data(vbmeta_file.to_str().unwrap()).len())
            .digest(read_test_data_as_str(vbmeta_digest).strip_suffix("\n").unwrap())
            .public_key_digest(TEST_PUBLIC_KEY_DIGEST)
            .unlocked(unlocked)
            .color(color)
            .extra("androidboot.force_normal_boot=1\n")
            .extra(format!("androidboot.slot_suffix=_{slot}\n"))
            .extra("androidboot.gbl.version=0\n")
            .extra(format!("androidboot.gbl.build_number={BUILD_NUMBER}\n"))
            .extra(FakeGblOps::GBL_TEST_BOOTCONFIG)
            .extra(vendor_config);

        for name in ["boot", "vendor_boot", "init_boot", "dtbo", "dtb"].iter() {
            let file = vbmeta_file.with_extension(format!("{name}.digest.txt"));
            if include.is_include_str(name)
                && Path::new(format!("{TEST_DATA_PATH}/{}", file.to_str().unwrap()).as_str())
                    .exists()
            {
                builder = builder.partition_digest(
                    name,
                    read_test_data_as_str(file.to_str().unwrap()).strip_suffix("\n").unwrap(),
                );
            }
        }

        builder.build()
    }

    /// Helper for testing `android_load_verify_fixup` given a partition layout, target slot and
    /// custom device tree.
    fn test_android_load_verify_fixup(
        slot_nr: u8,
        partitions: &[(String, String)],
        unlock: bool,
        expected_kernel: &[u8],
        expected_ramdisk: &[u8],
        expected_bootconfig: &[u8],
        expected_bootargs: &str,
        expected_fdt_property: &[(&str, &CStr, Option<&[u8]>)],
    ) {
        let mut storage = FakeGblOpsStorage::default();
        let partitions = partitions.iter().map(|(l, r)| (CString::new(l.clone()).unwrap(), r));
        for (part, file) in partitions.filter(|(_, f)| !f.is_empty()) {
            storage.add_raw_device(&part, read_test_data(file));
        }
        let mut ops = FakeGblOps::new(&storage);
        let slot_suffix = char::from_u32('a' as u32 + slot_nr as u32).unwrap();
        ops.current_slot = Some(Ok(slot(slot_suffix)));
        ops.avb_ops.unlock_state = Ok(unlock);
        ops.avb_ops.rollbacks = HashMap::from([(TEST_ROLLBACK_INDEX_LOCATION, Ok(0))]);
        let mut out_color = None;
        let mut handler = |color,
                           _: Option<&CStr>,
                           _: Option<&[u8]>,
                           _: Option<&[u8]>,
                           _: Option<&[u8]>,
                           _: Option<&[u8]>,
                           _: Option<&[u8]>,
                           _: Option<&[u8]>| {
            out_color = Some(color);
            Ok(())
        };
        ops.avb_handle_verification_result = Some(&mut handler);
        ops.avb_key_validation_status = Some(Ok(KeyValidationStatus::Valid));

        let mut load_buffer = AlignedBuffer::new(64 * 1024 * 1024, KERNEL_ALIGNMENT);
        let (ramdisk, fdt, kernel, _) =
            android_load_verify_fixup(&mut ops, slot_nr, false, &mut load_buffer).unwrap();
        assert_eq!(kernel, expected_kernel);
        check_ramdisk(ramdisk, expected_ramdisk, expected_bootconfig);

        let fdt = Fdt::new(fdt).unwrap();
        // "linux,initrd-start/end" are updated.
        assert_eq!(
            fdt.get_property("/chosen", c"linux,initrd-start").unwrap(),
            (ramdisk.as_ptr() as usize).to_be_bytes(),
        );
        assert_eq!(
            fdt.get_property("/chosen", c"linux,initrd-end").unwrap(),
            (ramdisk.as_ptr() as usize + ramdisk.len()).to_be_bytes(),
        );

        // Commandlines are updated.
        assert_eq!(
            CStr::from_bytes_until_nul(fdt.get_property("/chosen", c"bootargs").unwrap()).unwrap(),
            CString::new(expected_bootargs).unwrap().as_c_str(),
        );

        // Fixup is applied.
        assert_eq!(fdt.get_property("/chosen", c"fixup").unwrap(), &[1]);

        // Other FDT properties are as expected.
        for (path, property, res) in expected_fdt_property {
            assert_eq!(
                fdt.get_property(&path, &property).ok(),
                res.clone(),
                "{path}:{property:?} value doesn't match"
            );
        }
    }

    /// Helper for testing that `android_load_verify_fixup` succeeds for the given partition setup
    /// in various locked/unlocked mode.
    fn test_android_load_verify_fixup_success(
        slot: char,
        partitions: &[(String, String)],
        vbmeta: &str,
        expected_kernel: &[u8],
        expected_ramdisk: &[u8],
        expected_vendor_bootconfig: &str,
        expected_bootargs: &str,
        expected_fdt_property: &[(&str, &CStr, Option<&[u8]>)],
    ) {
        let dtb = partitions.iter().any(|(name, _)| name.starts_with("dtb_"));
        let dtbo = partitions.iter().any(|(name, _)| name.starts_with("dtbo_"));
        let test_common = |unlock, color, vbmeta_file: &str| {
            let mut partitions = partitions.to_vec();
            partitions.push((format!("vbmeta_{slot}"), vbmeta_file.into()));
            test_android_load_verify_fixup(
                (u64::from(slot) - ('a' as u64)).try_into().unwrap(),
                &partitions,
                unlock,
                expected_kernel,
                expected_ramdisk,
                &make_expected_bootconfig(
                    vbmeta_file,
                    unlock,
                    color,
                    slot,
                    expected_vendor_bootconfig,
                    MakeExpectedBootconfigInclude { dtb, dtbo, ..Default::default() },
                ),
                expected_bootargs,
                expected_fdt_property,
            )
        };
        // AVB verification passes in locked mode.
        println!("\n---sub test: AVB passes, locked mode---\n");
        test_common(false, BootStateColor::Green, vbmeta);
        // AVB verification passes in unlocked mode.
        println!("\n---sub test: AVB passes, unlocked mode---\n");
        test_common(true, BootStateColor::Orange, vbmeta);
        // Uses a noop vbmeta image that always succeeds but doesn't verified any on disk images.
        // Tests that in unlocked mode, images will be loaded as usual.
        println!("\n---sub test: Noop vbmeta, unlocked mode---\n");
        // TODO(b/416000842): `android_load_verify_fixup` is not checking verification status of
        // individual preloaded partitions. It will proceed even when locked, which is incorrect.
        test_common(true, BootStateColor::Orange, "vbmeta_noop.img");
    }

    const EXPECTED_V2_CMDLINE: &str = "existing_arg_1=existing_val_1 existing_arg_2=existing_val_2 cmd_key_1=cmd_val_1,cmd_key_2=cmd_val_2";

    /// Helper for testing `android_load_verify_fixup` for v2 boot image or lower.
    fn test_android_load_verify_fixup_v2_or_lower(
        ver: u8,
        slot: char,
        additional_parts: &[(&str, &str)],
        additional_expected_fdt_properties: &[(&str, &CStr, Option<&[u8]>)],
    ) {
        let vbmeta_file = format!("vbmeta_v{ver}_{slot}.img");
        let mut parts = vec![(format!("boot_{slot}"), format!("boot_v{ver}_{slot}.img"))];
        for (part, file) in additional_parts.iter().cloned() {
            parts.push((part.into(), file.into()));
        }
        test_android_load_verify_fixup_success(
            slot,
            &parts,
            &vbmeta_file,
            &read_test_data(format!("kernel_{slot}.img")),
            &read_test_data(format!("generic_ramdisk_{slot}.img")),
            "",
            EXPECTED_V2_CMDLINE,
            additional_expected_fdt_properties,
        )
    }

    #[test]
    fn test_android_load_verify_fixup_v0_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"dtb_slot", Some(b"a\0"))];
        // V0 image doesn't have built-in dtb. We need to provide from dtb partition.
        let parts = &[("dtb_a", "dtb_a.img")];
        test_android_load_verify_fixup_v2_or_lower(0, 'a', parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v0_dtbo_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"dtb_slot", Some(b"a\0")),
            ("/chosen", c"overlay_a_property", Some(b"overlay_a_val\0")),
        ];
        let parts = &[("dtbo_a", "dtbo_a.img"), ("dtb_a", "dtb_a.img")];
        test_android_load_verify_fixup_v2_or_lower(0, 'a', parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v0_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"dtb_slot", Some(b"b\0"))];
        let parts = &[("dtb_b", "dtb_b.img")];
        test_android_load_verify_fixup_v2_or_lower(0, 'b', parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v0_dtbo_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"dtb_slot", Some(b"b\0")),
            ("/chosen", c"overlay_b_property", Some(b"overlay_b_val\0")),
        ];
        let parts = &[("dtbo_b", "dtbo_b.img"), ("dtb_b", "dtb_b.img")];
        test_android_load_verify_fixup_v2_or_lower(0, 'b', parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v1_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"dtb_slot", Some(b"a\0"))];
        // V1 image doesn't have built-in dtb. We need to provide from dtb partition.
        let parts = &[("dtb_a", "dtb_a.img")];
        test_android_load_verify_fixup_v2_or_lower(1, 'a', parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v1_dtbo_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"dtb_slot", Some(b"a\0")),
            ("/chosen", c"overlay_a_property", Some(b"overlay_a_val\0")),
        ];
        let parts = &[("dtbo_a", "dtbo_a.img"), ("dtb_a", "dtb_a.img")];
        test_android_load_verify_fixup_v2_or_lower(1, 'a', parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v1_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"dtb_slot", Some(b"b\0"))];
        let parts = &[("dtb_b", "dtb_b.img")];
        test_android_load_verify_fixup_v2_or_lower(1, 'b', parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v1_dtbo_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"dtb_slot", Some(b"b\0")),
            ("/chosen", c"overlay_b_property", Some(b"overlay_b_val\0")),
        ];
        let parts = &[("dtbo_b", "dtbo_b.img"), ("dtb_b", "dtb_b.img")];
        test_android_load_verify_fixup_v2_or_lower(1, 'b', parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v2_slot_a() {
        // V2 image has built-in dtb. We don't need to provide custom device tree.
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        test_android_load_verify_fixup_v2_or_lower(2, 'a', &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v2_dtbo_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_a_property", Some(b"overlay_a_val\0")),
        ];
        let parts = &[("dtbo_a".into(), "dtbo_a.img".into())];
        test_android_load_verify_fixup_v2_or_lower(2, 'a', parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v2_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        test_android_load_verify_fixup_v2_or_lower(2, 'b', &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v2_dtbo_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_b_property", Some(b"overlay_b_val\0")),
        ];
        let parts = &[("dtbo_b".into(), "dtbo_b.img".into())];
        test_android_load_verify_fixup_v2_or_lower(2, 'b', parts, fdt_prop);
    }

    /// Returns the expected ramdisk for a slotted v3/v4 image in this test module.
    fn expected_v3_v4_ramdisk(slot: char) -> Vec<u8> {
        [
            read_test_data(format!("vendor_ramdisk_{slot}.img")),
            read_test_data(format!("generic_ramdisk_{slot}.img")),
        ]
        .concat()
    }

    const EXPECTED_V3_V4_CMDLINE: &str = "existing_arg_1=existing_val_1 existing_arg_2=existing_val_2 cmd_key_1=cmd_val_1,cmd_key_2=cmd_val_2 cmd_vendor_key_1=cmd_vendor_val_1,cmd_vendor_key_2=cmd_vendor_val_2";

    /// Common helper for testing `android_load_verify_fixup` for v3/v4 boot image.
    fn test_android_load_verify_fixup_v3_or_v4(
        slot: char,
        partitions: &[(String, String)],
        vbmeta_file: &str,
        expected_vendor_bootconfig: &str,
        additional_expected_fdt_properties: &[(&str, &CStr, Option<&[u8]>)],
    ) {
        test_android_load_verify_fixup_success(
            slot,
            partitions,
            vbmeta_file,
            &read_test_data(format!("kernel_{slot}.img")),
            &expected_v3_v4_ramdisk(slot),
            expected_vendor_bootconfig,
            EXPECTED_V3_V4_CMDLINE,
            additional_expected_fdt_properties,
        )
    }

    /// Helper for testing `android_load_verify_fixup` for v3/v4 boot image without init_boot.
    fn test_android_load_verify_fixup_v3_or_v4_no_init_boot(
        boot_ver: u32,
        vendor_ver: u32,
        slot: char,
        expected_vendor_bootconfig: &str,
        additional_parts: &[(String, String)],
        additional_expected_fdt_properties: &[(&str, &CStr, Option<&[u8]>)],
    ) {
        let vbmeta = format!("vbmeta_v{boot_ver}_v{vendor_ver}_{slot}.img");
        let mut parts = vec![
            (format!("boot_{slot}"), format!("boot_v{boot_ver}_{slot}.img")),
            (format!("vendor_boot_{slot}"), format!("vendor_boot_v{vendor_ver}_{slot}.img")),
        ];
        parts.extend_from_slice(additional_parts);
        test_android_load_verify_fixup_v3_or_v4(
            slot,
            &parts,
            &vbmeta,
            expected_vendor_bootconfig,
            additional_expected_fdt_properties,
        );
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v3_no_init_boot_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(3, 3, 'a', "", &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v3_no_init_boot_dtbo_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_a_property", Some(b"overlay_a_val\0")),
        ];
        let parts = &[("dtbo_a".into(), "dtbo_a.img".into())];
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(3, 3, 'a', "", parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v3_no_init_boot_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(3, 3, 'a', "", &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v3_no_init_boot_dtbo_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_b_property", Some(b"overlay_b_val\0")),
        ];
        let parts = &[("dtbo_b".into(), "dtbo_b.img".into())];
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(3, 3, 'b', "", parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v3_no_init_boot_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(4, 3, 'a', "", &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v3_no_init_boot_dtbo_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_a_property", Some(b"overlay_a_val\0")),
        ];
        let parts = &[("dtbo_a".into(), "dtbo_a.img".into())];
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(4, 3, 'a', "", parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v3_no_init_boot_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(4, 3, 'a', "", &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v3_no_init_boot_dtbo_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_b_property", Some(b"overlay_b_val\0")),
        ];
        let parts = &[("dtbo_b".into(), "dtbo_b.img".into())];
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(4, 3, 'b', "", parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v4_no_init_boot_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(3, 4, 'a', config, &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v4_no_init_boot_dtbo_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_a_property", Some(b"overlay_a_val\0")),
        ];
        let parts = &[("dtbo_a".into(), "dtbo_a.img".into())];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(3, 4, 'a', config, parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v4_no_init_boot_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(3, 4, 'a', config, &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v4_no_init_boot_dtbo_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_b_property", Some(b"overlay_b_val\0")),
        ];
        let parts = &[("dtbo_b".into(), "dtbo_b.img".into())];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(3, 4, 'b', config, parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v4_no_init_boot_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(4, 4, 'a', config, &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v4_no_init_boot_dtbo_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_a_property", Some(b"overlay_a_val\0")),
        ];
        let parts = &[("dtbo_a".into(), "dtbo_a.img".into())];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(4, 4, 'a', config, parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v4_no_init_boot_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(4, 4, 'a', config, &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v4_no_init_boot_dtbo_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_b_property", Some(b"overlay_b_val\0")),
        ];
        let parts = &[("dtbo_b".into(), "dtbo_b.img".into())];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_no_init_boot(4, 4, 'b', config, parts, fdt_prop);
    }

    /// Helper for testing `android_load_verify_fixup` with dttable vendor_boot
    fn test_android_load_verify_fixup_v4_vendor_boot_dttable(
        slot: char,
        expected_vendor_bootconfig: &str,
        additional_parts: &[(String, String)],
        additional_expected_fdt_properties: &[(&str, &CStr, Option<&[u8]>)],
    ) {
        let vbmeta = format!("vbmeta_v4_dttable_{slot}.img");
        let mut parts = vec![
            (format!("boot_{slot}"), format!("boot_v4_{slot}.img")),
            (format!("vendor_boot_{slot}"), format!("vendor_boot_v4_dttable_{slot}.img")),
        ];
        parts.extend_from_slice(additional_parts);
        test_android_load_verify_fixup_v3_or_v4(
            slot,
            &parts,
            &vbmeta,
            expected_vendor_bootconfig,
            additional_expected_fdt_properties,
        );
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v4_no_init_boot_slot_dttable_vendor_boot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v4_vendor_boot_dttable('a', config, &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v4_no_init_boot_slot_dttable_vendor_boot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v4_vendor_boot_dttable('b', config, &[], fdt_prop);
    }

    /// Helper for testing `android_load_verify_fixup` for v3/v4 boot image with init_boot.
    fn test_android_load_verify_fixup_v3_or_v4_init_boot(
        boot_ver: u32,
        vendor_ver: u32,
        slot: char,
        expected_vendor_bootconfig: &str,
        additional_parts: &[(String, String)],
        additional_expected_fdt_properties: &[(&str, &CStr, Option<&[u8]>)],
    ) {
        let vbmeta = format!("vbmeta_v{boot_ver}_v{vendor_ver}_init_boot_{slot}.img");
        let mut parts = vec![
            (format!("boot_{slot}"), format!("boot_no_ramdisk_v{boot_ver}_{slot}.img")),
            (format!("vendor_boot_{slot}"), format!("vendor_boot_v{vendor_ver}_{slot}.img")),
            (format!("init_boot_{slot}"), format!("init_boot_{slot}.img")),
        ];
        parts.extend_from_slice(additional_parts);
        test_android_load_verify_fixup_v3_or_v4(
            slot,
            &parts,
            &vbmeta,
            expected_vendor_bootconfig,
            additional_expected_fdt_properties,
        );
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v3_init_boot_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        test_android_load_verify_fixup_v3_or_v4_init_boot(3, 3, 'a', "", &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v3_init_boot_dtbo_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_a_property", Some(b"overlay_a_val\0")),
        ];
        let parts = &[("dtbo_a".into(), "dtbo_a.img".into())];
        test_android_load_verify_fixup_v3_or_v4_init_boot(3, 3, 'a', "", parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v3_init_boot_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        test_android_load_verify_fixup_v3_or_v4_init_boot(3, 3, 'a', "", &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v3_init_boot_dtbo_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_b_property", Some(b"overlay_b_val\0")),
        ];
        let parts = &[("dtbo_b".into(), "dtbo_b.img".into())];
        test_android_load_verify_fixup_v3_or_v4_init_boot(3, 3, 'b', "", parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v3_init_boot_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        test_android_load_verify_fixup_v3_or_v4_init_boot(4, 3, 'a', "", &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v3_init_boot_dtbo_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_a_property", Some(b"overlay_a_val\0")),
        ];
        let parts = &[("dtbo_a".into(), "dtbo_a.img".into())];
        test_android_load_verify_fixup_v3_or_v4_init_boot(4, 3, 'a', "", parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v3_init_boot_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        test_android_load_verify_fixup_v3_or_v4_init_boot(4, 3, 'a', "", &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v3_init_boot_dtbo_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_b_property", Some(b"overlay_b_val\0")),
        ];
        let parts = &[("dtbo_b".into(), "dtbo_b.img".into())];
        test_android_load_verify_fixup_v3_or_v4_init_boot(4, 3, 'b', "", parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v4_init_boot_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_init_boot(3, 4, 'a', config, &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v4_init_boot_dtbo_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_a_property", Some(b"overlay_a_val\0")),
        ];
        let parts = &[("dtbo_a".into(), "dtbo_a.img".into())];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_init_boot(3, 4, 'a', config, parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v4_init_boot_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_init_boot(3, 4, 'a', config, &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v3_v4_init_boot_dtbo_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_b_property", Some(b"overlay_b_val\0")),
        ];
        let parts = &[("dtbo_b".into(), "dtbo_b.img".into())];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_init_boot(3, 4, 'b', config, parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v4_init_boot_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_init_boot(4, 4, 'a', config, &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v4_init_boot_dtbo_slot_a() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_a_property", Some(b"overlay_a_val\0")),
        ];
        let parts = &[("dtbo_a".into(), "dtbo_a.img".into())];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_init_boot(4, 4, 'a', config, parts, fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v4_init_boot_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[("/chosen", c"builtin", Some(&[1]))];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_init_boot(4, 4, 'a', config, &[], fdt_prop);
    }

    #[test]
    fn test_android_load_verify_fixup_v4_v4_init_boot_dtbo_slot_b() {
        let fdt_prop: &[(&str, &CStr, Option<&[u8]>)] = &[
            ("/chosen", c"builtin", Some(&[1])),
            ("/chosen", c"overlay_b_property", Some(b"overlay_b_val\0")),
        ];
        let parts = &[("dtbo_b".into(), "dtbo_b.img".into())];
        let config = TEST_VENDOR_BOOTCONFIG;
        test_android_load_verify_fixup_v3_or_v4_init_boot(4, 4, 'b', config, parts, fdt_prop);
    }

    /// Helper for testing v4 boot image with different kernel compression.
    fn test_android_load_verify_boot_v4_compression_slot(compression: &str) {
        let vbmeta = format!("vbmeta_v4_{compression}_a.img");
        let parts = vec![
            (format!("boot_a"), format!("boot_v4_{compression}_a.img")),
            (format!("vendor_boot_a"), format!("vendor_boot_v4_a.img")),
            (format!("vbmeta_a"), vbmeta.clone()),
        ];
        test_android_load_verify_fixup(
            0,
            &parts,
            false,
            &read_test_data(format!("gki_boot_{compression}_kernel_uncompressed")),
            &expected_v3_v4_ramdisk('a'),
            &make_expected_bootconfig(
                &vbmeta,
                false,
                BootStateColor::Green,
                'a',
                TEST_VENDOR_BOOTCONFIG,
                MakeExpectedBootconfigInclude { dtbo: false, dtb: false, ..Default::default() },
            ),
            EXPECTED_V3_V4_CMDLINE,
            &[],
        )
    }

    #[test]
    fn test_android_load_verify_gzip_boot_v4_vendor_v4_slot_a() {
        test_android_load_verify_boot_v4_compression_slot("gz")
    }

    #[test]
    fn test_android_load_verify_lz4_boot_v4_vendor_v4_slot_a() {
        test_android_load_verify_boot_v4_compression_slot("lz4")
    }

    /// Helper for checking V2 image loaded from slot A and in normal mode.
    pub(crate) fn checks_loaded_v2_slot_a_normal_mode(ramdisk: &[u8], kernel: &[u8]) {
        let expected_bootconfig = AvbResultBootconfigBuilder::new()
            .vbmeta_size(read_test_data("vbmeta_v2_a.img").len())
            .digest(read_test_data_as_str("vbmeta_v2_a.digest.txt").strip_suffix("\n").unwrap())
            .partition_digest(
                "boot",
                read_test_data_as_str("vbmeta_v2_a.boot.digest.txt").strip_suffix("\n").unwrap(),
            )
            .public_key_digest(TEST_PUBLIC_KEY_DIGEST)
            .extra("androidboot.force_normal_boot=1\n")
            .extra(format!("androidboot.slot_suffix=_a\n"))
            .extra("androidboot.gbl.version=0\n")
            .extra(format!("androidboot.gbl.build_number={BUILD_NUMBER}\n"))
            .extra(FakeGblOps::GBL_TEST_BOOTCONFIG)
            .build();
        check_ramdisk(ramdisk, &read_test_data("generic_ramdisk_a.img"), &expected_bootconfig);
        assert_eq!(kernel, read_test_data("kernel_a.img"));
    }

    /// Helper for checking V2 image loaded from slot A and in recovery mode.
    fn checks_loaded_v2_slot_a_recovery_mode(ramdisk: &[u8], kernel: &[u8]) {
        let expected_bootconfig = AvbResultBootconfigBuilder::new()
            .vbmeta_size(read_test_data("vbmeta_v2_a.img").len())
            .digest(read_test_data_as_str("vbmeta_v2_a.digest.txt").strip_suffix("\n").unwrap())
            .partition_digest(
                "boot",
                read_test_data_as_str("vbmeta_v2_a.boot.digest.txt").strip_suffix("\n").unwrap(),
            )
            .public_key_digest(TEST_PUBLIC_KEY_DIGEST)
            .extra(format!("androidboot.slot_suffix=_a\n"))
            .extra("androidboot.gbl.version=0\n")
            .extra(format!("androidboot.gbl.build_number={BUILD_NUMBER}\n"))
            .extra(FakeGblOps::GBL_TEST_BOOTCONFIG)
            .build();
        check_ramdisk(ramdisk, &read_test_data("generic_ramdisk_a.img"), &expected_bootconfig);
        assert_eq!(kernel, read_test_data("kernel_a.img"));
    }

    /// Helper for getting default FakeGblOps for tests.
    pub(crate) fn default_test_gbl_ops(storage: &FakeGblOpsStorage) -> FakeGblOps {
        let mut ops = FakeGblOps::new(&storage);
        ops.avb_ops.unlock_state = Ok(false);
        ops.avb_ops.rollbacks = HashMap::from([(TEST_ROLLBACK_INDEX_LOCATION, Ok(0))]);
        ops.avb_key_validation_status = Some(Ok(KeyValidationStatus::Valid));
        ops.current_slot = Some(Ok(slot('a')));
        ops.reboot_reason = Some(Ok(RebootReason::Normal));
        ops
    }

    #[test]
    fn test_android_load_verify_fixup_recovery_mode() {
        // Recovery mode is specified by the absence of bootconfig arg
        // "androidboot.force_normal_boot=1\n" and therefore independent of image versions. We can
        // pick any image version for test. Use v2 for simplicity.
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_a", read_test_data("boot_v2_a.img"));
        storage.add_raw_device(c"vbmeta_a", read_test_data("vbmeta_v2_a.img"));

        let mut ops = default_test_gbl_ops(&storage);
        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        let (ramdisk, _, kernel, _) =
            android_load_verify_fixup(&mut ops, 0, true, &mut load_buffer).unwrap();
        checks_loaded_v2_slot_a_recovery_mode(ramdisk, kernel)
    }

    #[test]
    fn test_android_main_bcb_normal_mode() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_a", read_test_data("boot_v2_a.img"));
        storage.add_raw_device(c"vbmeta_a", read_test_data("vbmeta_v2_a.img"));
        storage.add_raw_device(c"misc", vec![0u8; 4 * 1024 * 1024]);

        let mut ops = default_test_gbl_ops(&storage);
        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        let (ramdisk, _, kernel, _) = android_main(&mut ops, &mut load_buffer, |_| {}).unwrap();
        checks_loaded_v2_slot_a_normal_mode(ramdisk, kernel)
    }

    #[test]
    fn test_android_main_bcb_recovery_mode() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_a", read_test_data("boot_v2_a.img"));
        storage.add_raw_device(c"vbmeta_a", read_test_data("vbmeta_v2_a.img"));
        storage.add_raw_device(c"misc", vec![0u8; 4 * 1024 * 1024]);

        let mut ops = default_test_gbl_ops(&storage);
        ops.write_to_partition_sync("misc", 0, &mut b"boot-recovery".to_vec()).unwrap();
        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        let (ramdisk, _, kernel, _) = android_main(&mut ops, &mut load_buffer, |_| {}).unwrap();
        checks_loaded_v2_slot_a_recovery_mode(ramdisk, kernel)
    }

    #[test]
    fn test_android_main_reboot_reason_recovery_mode() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_a", read_test_data("boot_v2_a.img"));
        storage.add_raw_device(c"vbmeta_a", read_test_data("vbmeta_v2_a.img"));
        storage.add_raw_device(c"misc", vec![0u8; 4 * 1024 * 1024]);

        let mut ops = default_test_gbl_ops(&storage);
        ops.reboot_reason = Some(Ok(RebootReason::Recovery));
        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        let (ramdisk, _, kernel, _) = android_main(&mut ops, &mut load_buffer, |_| {}).unwrap();
        checks_loaded_v2_slot_a_recovery_mode(ramdisk, kernel)
    }

    /// Helper for checking V2 image loaded from slot B and in normal mode.
    pub(crate) fn checks_loaded_v2_slot_b_normal_mode(ramdisk: &[u8], kernel: &[u8]) {
        let expected_bootconfig = AvbResultBootconfigBuilder::new()
            .vbmeta_size(read_test_data("vbmeta_v2_b.img").len())
            .digest(read_test_data_as_str("vbmeta_v2_b.digest.txt").strip_suffix("\n").unwrap())
            .partition_digest(
                "boot",
                read_test_data_as_str("vbmeta_v2_b.boot.digest.txt").strip_suffix("\n").unwrap(),
            )
            .public_key_digest(TEST_PUBLIC_KEY_DIGEST)
            .extra("androidboot.force_normal_boot=1\n")
            .extra(format!("androidboot.slot_suffix=_b\n"))
            .extra("androidboot.gbl.version=0\n")
            .extra(format!("androidboot.gbl.build_number={BUILD_NUMBER}\n"))
            .extra(FakeGblOps::GBL_TEST_BOOTCONFIG)
            .build();
        check_ramdisk(ramdisk, &read_test_data("generic_ramdisk_b.img"), &expected_bootconfig);
        assert_eq!(kernel, read_test_data("kernel_b.img"));
    }

    #[test]
    fn test_android_main_slotted_gbl_slot_a() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_a", read_test_data("boot_v2_a.img"));
        storage.add_raw_device(c"vbmeta_a", read_test_data("vbmeta_v2_a.img"));
        storage.add_raw_device(c"misc", vec![0u8; 4 * 1024 * 1024]);

        let mut ops = default_test_gbl_ops(&storage);
        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        let (ramdisk, _, kernel, _) = android_main(&mut ops, &mut load_buffer, |_| {}).unwrap();
        assert_eq!(ops.mark_boot_attempt_called, 0);
        checks_loaded_v2_slot_a_normal_mode(ramdisk, kernel)
    }

    #[test]
    fn test_android_main_slotless_gbl_slot_a() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_a", read_test_data("boot_v2_a.img"));
        storage.add_raw_device(c"vbmeta_a", read_test_data("vbmeta_v2_a.img"));
        storage.add_raw_device(c"misc", vec![0u8; 4 * 1024 * 1024]);

        let mut ops = default_test_gbl_ops(&storage);
        ops.current_slot = Some(Err(Error::Unsupported));
        ops.next_slot = Some(Ok(slot('a')));
        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        let (ramdisk, _, kernel, _) = android_main(&mut ops, &mut load_buffer, |_| {}).unwrap();
        assert_eq!(ops.mark_boot_attempt_called, 1);
        checks_loaded_v2_slot_a_normal_mode(ramdisk, kernel)
    }

    #[test]
    fn test_android_main_slotted_gbl_slot_b() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_b", read_test_data("boot_v2_b.img"));
        storage.add_raw_device(c"vbmeta_b", read_test_data("vbmeta_v2_b.img"));
        storage.add_raw_device(c"misc", vec![0u8; 4 * 1024 * 1024]);

        let mut ops = default_test_gbl_ops(&storage);
        ops.current_slot = Some(Ok(slot('b')));

        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        let (ramdisk, _, kernel, _) = android_main(&mut ops, &mut load_buffer, |_| {}).unwrap();
        assert_eq!(ops.mark_boot_attempt_called, 0);
        checks_loaded_v2_slot_b_normal_mode(ramdisk, kernel)
    }

    #[test]
    fn test_android_main_slotless_gbl_slot_b() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_b", read_test_data("boot_v2_b.img"));
        storage.add_raw_device(c"vbmeta_b", read_test_data("vbmeta_v2_b.img"));
        storage.add_raw_device(c"misc", vec![0u8; 4 * 1024 * 1024]);

        let mut ops = default_test_gbl_ops(&storage);
        ops.current_slot = Some(Err(Error::Unsupported));
        ops.next_slot = Some(Ok(slot('b')));
        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        let (ramdisk, _, kernel, _) = android_main(&mut ops, &mut load_buffer, |_| {}).unwrap();
        assert_eq!(ops.mark_boot_attempt_called, 1);
        checks_loaded_v2_slot_b_normal_mode(ramdisk, kernel);
    }

    #[test]
    fn test_android_main_unsupported_slot_default_to_a() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_a", read_test_data("boot_v2_a.img"));
        storage.add_raw_device(c"vbmeta_a", read_test_data("vbmeta_v2_a.img"));
        storage.add_raw_device(c"misc", vec![0u8; 4 * 1024 * 1024]);

        let mut ops = default_test_gbl_ops(&storage);
        ops.current_slot = Some(Err(Error::Unsupported));
        ops.next_slot = Some(Err(Error::Unsupported));
        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        let (ramdisk, _, kernel, _) = android_main(&mut ops, &mut load_buffer, |_| {}).unwrap();
        checks_loaded_v2_slot_a_normal_mode(ramdisk, kernel)
    }

    /// Helper for testing that fastboot mode is triggered.
    fn test_fastboot_is_triggered<'a, 'b>(ops: &mut impl GblOps<'a, 'b>) {
        let listener: SharedTestListener = Default::default();
        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        let (ramdisk, _, kernel, _) = android_main(ops, &mut load_buffer, |fb| {
            listener.add_usb_input(b"getvar:max-fetch-size");
            listener.add_usb_input(b"continue");
            fb.run_n::<2>(
                &mut vec![0u8; 256 * 1024],
                Some(&mut TestLocalSession::default()),
                Some(&listener),
                Some(&listener),
            )
        })
        .unwrap();

        assert_eq!(
            listener.usb_out_queue(),
            make_expected_usb_out(
                &[b"OKAY0xffffffffffffffff", b"INFOSyncing storage...", b"OKAY",]
            ),
            "\nActual USB output:\n{}",
            listener.dump_usb_out_queue()
        );

        checks_loaded_v2_slot_a_normal_mode(ramdisk, kernel);
    }

    #[test]
    fn test_android_main_bootonce_bootloader_bcb_command_is_cleared() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_a", read_test_data("boot_v2_a.img"));
        storage.add_raw_device(c"vbmeta_a", read_test_data("vbmeta_v2_a.img"));
        storage.add_raw_device(c"misc", vec![0u8; 4 * 1024 * 1024]);
        let mut ops = default_test_gbl_ops(&storage);
        ops.write_to_partition_sync("misc", 0, &mut b"bootonce-bootloader".to_vec()).unwrap();
        test_fastboot_is_triggered(&mut ops);

        let mut bcb_buffer = [0u8; BootloaderMessage::SIZE_BYTES];
        ops.read_from_partition_sync("misc", 0, &mut bcb_buffer[..]).unwrap();
        let bcb = BootloaderMessage::from_bytes_ref(&bcb_buffer).unwrap();
        assert_eq!(
            bcb.boot_mode().unwrap(),
            AndroidBootMode::Normal,
            "BCB mode is expected to be cleared after bootonce-bootloader is handled"
        );
    }

    #[test]
    fn test_android_main_enter_fastboot_via_bcb() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_a", read_test_data("boot_v2_a.img"));
        storage.add_raw_device(c"vbmeta_a", read_test_data("vbmeta_v2_a.img"));
        storage.add_raw_device(c"misc", vec![0u8; 4 * 1024 * 1024]);
        let mut ops = default_test_gbl_ops(&storage);
        ops.write_to_partition_sync("misc", 0, &mut b"bootonce-bootloader".to_vec()).unwrap();
        test_fastboot_is_triggered(&mut ops);
    }

    #[test]
    fn test_android_main_enter_fastboot_via_reboot_reason() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_a", read_test_data("boot_v2_a.img"));
        storage.add_raw_device(c"vbmeta_a", read_test_data("vbmeta_v2_a.img"));
        storage.add_raw_device(c"misc", vec![0u8; 4 * 1024 * 1024]);
        let mut ops = default_test_gbl_ops(&storage);
        ops.reboot_reason = Some(Ok(RebootReason::Bootloader));
        test_fastboot_is_triggered(&mut ops);
    }

    #[test]
    fn test_android_main_enter_fastboot_via_should_stop_in_fastboot() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"boot_a", read_test_data("boot_v2_a.img"));
        storage.add_raw_device(c"vbmeta_a", read_test_data("vbmeta_v2_a.img"));
        storage.add_raw_device(c"misc", vec![0u8; 4 * 1024 * 1024]);
        let mut ops = default_test_gbl_ops(&storage);
        ops.stop_in_fastboot = Some(Ok(true));
        test_fastboot_is_triggered(&mut ops);
    }

    #[test]
    fn test_android_main_fastboot_boot() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"vbmeta_a", read_test_data("vbmeta_v2_a.img"));
        storage.add_raw_device(c"misc", vec![0u8; 4 * 1024 * 1024]);
        let mut ops = default_test_gbl_ops(&storage);
        ops.stop_in_fastboot = Some(Ok(true));
        ops.current_slot = Some(Ok(slot('a')));

        let listener: SharedTestListener = Default::default();
        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        let (ramdisk, _, kernel, _) = android_main(&mut ops, &mut load_buffer, |fb| {
            let data = read_test_data(format!("boot_v2_a.img"));
            listener.add_usb_input(format!("download:{:#x}", data.len()).as_bytes());
            listener.add_usb_input(&data);
            listener.add_usb_input(b"boot");
            listener.add_usb_input(b"continue");
            fb.run_n::<2>(
                &mut vec![0u8; 256 * 1024],
                Some(&mut TestLocalSession::default()),
                Some(&listener),
                Some(&listener),
            )
        })
        .unwrap();

        assert_eq!(
            listener.usb_out_queue(),
            make_expected_usb_out(&[
                b"DATA00004000",
                b"OKAY",
                b"INFOBoot image as Android slot a",
                b"OKAY",
            ]),
            "\nActual USB output:\n{}",
            listener.dump_usb_out_queue()
        );

        checks_loaded_v2_slot_a_normal_mode(ramdisk, kernel);
    }

    #[test]
    fn test_android_main_reboot_if_set_active_to_different_slot() {
        let mut storage = FakeGblOpsStorage::default();
        storage.add_raw_device(c"misc", vec![0u8; 4 * 1024 * 1024]);
        let mut ops = default_test_gbl_ops(&storage);
        ops.stop_in_fastboot = Some(Ok(true));
        ops.current_slot = Some(Ok(slot('a')));

        let listener: SharedTestListener = Default::default();
        let mut load_buffer = AlignedBuffer::new(8 * 1024 * 1024, KERNEL_ALIGNMENT);
        assert_eq!(
            android_main(&mut ops, &mut load_buffer, |fb| {
                listener.add_usb_input(b"set_active:b");
                listener.add_usb_input(b"continue");
                fb.run_n::<2>(
                    &mut vec![0u8; 256 * 1024],
                    Some(&mut TestLocalSession::default()),
                    Some(&listener),
                    Some(&listener),
                )
            })
            .unwrap_err(),
            Error::UnexpectedReturn.into()
        );

        assert_eq!(
            listener.usb_out_queue(),
            make_expected_usb_out(&[b"OKAY", b"INFOSyncing storage...", b"OKAY",]),
            "\nActual USB output:\n{}",
            listener.dump_usb_out_queue()
        );
    }
}
