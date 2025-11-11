# Generic Bootloader Library

This directory hosts the Generic Bootloader Library project. A Bazel
workspace is setup for building the library as well as an EFI executable that
can be loaded directly from the firmware.

## Get source tree and build

### Prerequisites

The GBL build currently only supports Linux x86_64 host machines.

Your machine must have the following dependencies installed:

* `repo` to work with android repositories
  (https://source.android.com/docs/setup/reference/repo)
* `bazel-bootstrap` to build (https://bazel.build/)

On Google Linux machines these tools can be installed via:

```shell
sudo apt install repo bazel-bootstrap
```

### Download the source

Use `repo` to download the source using the Android
[uefi-gbl-mainline manifest](https://android.googlesource.com/kernel/manifest/+/refs/heads/uefi-gbl-mainline/default.xml):

```shell
# You can choose a different directory name if you prefer.
mkdir gbl
cd gbl

repo init -u https://android.googlesource.com/kernel/manifest -b uefi-gbl-mainline
repo sync -j16
```

### Build the UEFI applications

Run this command from the repo root directory (`gbl` in the example above):

```shell
./tools/bazel run //bootable/libbootloader:gbl_efi_dist
```

This command builds all variations of the EFI application (dev + prod for each
of `x86_64`, `aarch64`, and `riscv64` architectures). The application binaries
will be placed in `out/gbl_efi/`.

### Run host-side unittests

Run this command from the repo root directory (`gbl` in the example above):

```shell
./tools/bazel test @gbl//tests
```

## IDE Setup

For rust development, we recommend use VSCode + rust-analyzer plugin.

rust-analyzer requires `rust-project.json` to work properly. Luckily, bazel has
support for generating `rust-project.json`:

```
./tools/bazel run @rules_rust//tools/rust_analyzer:gen_rust_project --norepository_disable_download -- --bazel ./tools/bazel @gbl//efi/...
```

`@gbl//efi/...` is the target to generate rust project for, here it means
"everything under @gbl//efi/ directory" . Omitting the target specifier would
result in analyzing "@/..." , which would most likely fail due to some obscure
reason. Should targets get moved around in the future, this path spec also need
to be updated.

After generating `rust-project.json`, you would notice that your IDE still
doesn't offer auto completion. This is because some source file paths pointing
to bazel-output dir, and you are most likely editing source files in
`bootable/libbootloader/gbl`. In addition, the generated rust-project.json sets
"cfg=test" for all targets, which causes certain dependency graph to resolve
incorrectly. To fix this, run

```
python3 bootable/libbootloader/gbl/rewrite_rust_project_path.py rust-project.json --arch <arch>
```
where `<arch>` is the target architecture of interest and should be one of
`x86_64`, `aarch64`, `riscv64`. `<arch>` affects intellisense on architecture
specific code.

And reload your IDE.

## Gemini CLI

This repo can be used with
[Gemini CLI](https://github.com/google-gemini/gemini-cli). Refer to that
link for installation instructions.

To use it, run `gemini` from this directory so that it sees the `GEMINI.md`
file to help specialize it for this codebase.

## Run the EFI application

### Boot Android on Cuttlefish

If you have a main AOSP checkout and is setup to run
[Cuttlefish](https://source.android.com/docs/setup/create/cuttlefish), you can
run the EFI image directly with:

```
cvd create --android_efi_loader=<path to the EFI image> ...
```

The above uses the same setting as a normal `cvd create` run, except that
instead of booting Android directly, the emulator first hands off to the EFI
application, which will take over booting android.

Note: For x86 platform, use the EFI image built for `x86_64`.

### Boot Fuchsia on Vim3

Booting Fuchsia on a Vim3 development board is supported. To run the
application:

1. Complete all
[bootstrap steps](https://fuchsia.dev/fuchsia-src/development/hardware/khadas-vim3?hl=en)
to setup Vim3 as a Fuchsia device.
2. Reboot the device into fastboot mode.
3. Run fastboot command:
```
fastboot stage <path to the EFI binary> && fastboot oem run-staged-efi
```

### Run on standalone QEMU

If you want to test the EFI image directly on QEMU with your custom
configurations:

1. Install EDK, QEMU and u-boot prebuilts

   ```
   sudo apt-get install qemu-system ovmf u-boot-qemu
   ```

1. Depending on the target architecture you want to run:

   For `x86_64`:
   ```
   mkdir -p /tmp/esp/EFI/BOOT && \
   cp <path to EFI image> /tmp/esp/EFI/BOOT/bootx64.efi && \
   qemu-system-x86_64 -nographic -m 1G \
       -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd \
       -drive format=raw,file=fat:rw:/tmp/esp
   ```

   For `aarch64`:
   ```
   mkdir -p /tmp/esp/EFI/BOOT && \
   cp <path to EFI image> /tmp/esp/EFI/BOOT/bootaa64.efi && \
   qemu-system-aarch64 -nographic -machine virt -m 1G -cpu cortex-a57 \
       -drive if=pflash,format=raw,readonly=on,file=/usr/share/AAVMF/AAVMF_CODE.fd \
       -drive format=raw,file=fat:rw:/tmp/esp
   ```

   For `riscv64`:
   ```
   mkdir -p /tmp/esp/EFI/BOOT && \
   cp <path to EFI image> /tmp/esp/EFI/BOOT/bootriscv64.efi && \
   qemu-system-riscv64 -nographic -machine virt -m 1G \
       -bios /usr/lib/u-boot/qemu-riscv64/u-boot.bin \
       -drive format=raw,file=fat:rw:/tmp/esp
   ```

### Debug with GDB on QEMU

[qemu_gdb_example/](./qemu_gdb_example/) provides an example for
debugging x86_64 GBL EFI app on QEMU using rust-gdb. To try the example:

1. Install necessary dependencies:
   ```
   sudo apt-get install qemu-system ovmf
   ```

   For aarch64 target debugging, also install:
   ```
   sudo apt-get install gdb-multiarch
   ```

2. Runs the following script:
   ```
   ./qemu_gdb_example/launch_qemu_gdb.sh
   ```

   The above command builds a debug x86_64 GBL EFI app, launches it in QEMU
   and starts `rust-gdb` in a separate terminal for debugging.

   For debugging aarch64 target, run:
   ```
   ./qemu_gdb_example/launch_qemu_gdb.sh aarch64
   ```

### Debug with LLDB on Cuttlefish

For x86_64 and aarch64, a pdb file is built along the GBL EFI application.
The following gives an example of debugging GBL with the pdb file on Cuttlefish.
(Currently only aarch64 is supported.)

1. Build GBL with GDB connection listening enabled:

   ```
   ./tools/bazel run //bootable/libbootloader:gbl_efi_dist -c dbg --@gbl//toolchain:always_wait_gdb
   ```

2. Launch cuttlefish with GBL and qemu.

   ```
   launch_cvd \
      --vm_manager=qemu_cli \
      --android_efi_loader=./out/gbl_efi/gbl_aarch64_dev.efi \
      --gdb_port=1337 \
      --cpus=1
   ```

3. After cuttlefish emulator started successfully, in a separate terminal,
   launch lldb.

   ```
   lldb \
    -o "target create ./out/gbl_efi/gbl_aarch64_dev.efi" \
    -o "gdb-remote localhost:1337" \
    -o "command script import bootable/libbootloader/gbl/qemu_gdb_example/lldb_load_gbl.py" \
    -o "script lldb_load_gbl.start_gbl()"
   ```

### Boot Fuchsia on emulator

1. Make sure Fuchsia target pass control to GBL.

   Set path to GBL binary here: [fuchsia/src/firmware/gigaboot/cpp/backends.gni : gigaboot_gbl_efi_app](https://cs.opensource.google/fuchsia/fuchsia/+/main:src/firmware/gigaboot/cpp/backends.gni;l=25?q=gigaboot_gbl_efi_app)

   Temporarily  need to enable GBL usage in gigaboot: [fuchsia/src/firmware/gigaboot/cpp/backends.gni : gigaboot_use_gbl](https://cs.opensource.google/fuchsia/fuchsia/+/main:src/firmware/gigaboot/cpp/backends.gni;l=25?q=gigaboot_gbl_efi_app#:~:text=to%20use%20GBL.-,gigaboot_use_gbl)

   E.g. in `fuchsia/src/firmware/gigaboot/cpp/backends.gni`:
   ```
   $ cat ./fuchsia/src/firmware/gigaboot/cpp/backends.gni
   ...
   declare_args() {
      ...
      gigaboot_gbl_efi_app = "<path to EFI image>/gbl_x86_64.efi"
      gigaboot_use_gbl = true
   }
   ```

   Or in `fx set`:
   ```
   fx set core.x64 --args=gigaboot_gbl_efi_app='"<path to EFI image>/gbl_x86_64.efi"' --args=gigaboot_use_gbl=true
   ```

2. Build: (this has to be done every time if EFI app changes)

   `fx build`

3. Run emulator in UEFI mode with raw disk

   ```
   fx qemu -a x64 --uefi --disktype=nvme -D ./out/default/obj/build/images/disk.raw
   ```

## EFI Protocols

List of EFI protocols used by GBL and a brief description of each [here](./docs/efi_protocols.md).

## Licesing

Unless stated otherwise, all GBL source files are licensed under the Apache
License, Version 2.0.

UEFI definitions, along with UEFI, community, and GBL-specific protocol headers
located in `libefi_types/defs/**/*.h`, may alternatively be used under the
BSD-2-Clause-Patent license.

See `../LICENSES/Apache-2.0.txt` and `../LICENSES/BSD-2-Clause-Patents.txt` for
the full texts.
