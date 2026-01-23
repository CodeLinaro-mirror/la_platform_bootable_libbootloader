# Buffer Usage in GBL

This doc discusses memory buffer usage by GBL.

## Boot Buffers

At run time, GBL requests the following memory buffers via
[GBL_EFI_BOOT_MEMORY_PROTOCOL.GetBootBuffer()](./gbl_efi_boot_memory_protocol.md#gbl_efi_boot_memory_protocolgetbootbuffer)
for assembling various boot images such as kernel, ramdisk, FDT and pvmfw.

### General Load Buffer

GBL uses this buffer for:

1. Assembling images that do not have a dedicated load buffer. (See below for
   image specific load buffers).
2. Temporary scracth buffers for reading BCB boot mode, relocating DT components
   etc.

GBL queries the buffer by calling
[GBL_EFI_BOOT_MEMORY_PROTOCOL.GetBootBuffer()](./gbl_efi_boot_memory_protocol.md#gbl_efi_boot_memory_protocolgetbootbuffer)
with parameter `BootBufferType` set to `GENERAL_LOAD`. If buffer is not
provided, GBL allocates a default 256MB memory via EFI allocation.

### Kernel Load Buffer

GBL uses this buffer for placing the kernel image (after decompression) from the
Android Boot image. This will also be the buffer GBL boots OS from.

GBL queries the buffer by calling
[GBL_EFI_BOOT_MEMORY_PROTOCOL.GetBootBuffer()](./gbl_efi_boot_memory_protocol.md#gbl_efi_boot_memory_protocolgetbootbuffer)
with parameter `BootBufferType` set to `KERNEL`. The buffer is optional. If not
provided, GBL will look for space from the general load buffer.

### Ramdisk Load Buffer

GBL uses this buffer for assembling ramdisk image for boot.

GBL queries the buffer by calling
[GBL_EFI_BOOT_MEMORY_PROTOCOL.GetBootBuffer()](./gbl_efi_boot_memory_protocol.md#gbl_efi_boot_memory_protocolgetbootbuffer)
with parameter `BootBufferType` set to `RAMDISK`. The buffer is optional. If not
provided, GBL will look for space from the general load buffer.

### FDT Load Buffer

GBL uses this buffer for constructing and fixing up the final FDT for boot.

GBL queries the buffer by calling
[GBL_EFI_BOOT_MEMORY_PROTOCOL.GetBootBuffer()](./gbl_efi_boot_memory_protocol.md#gbl_efi_boot_memory_protocolgetbootbuffer)
with parameter `BootBufferType` set to `FDT`. The buffer is optional. If not
provided, GBL will look for space from the general load buffer.

### Pvmfw Load Buffer

GBL uses this buffer assembling the pvmfw image.

GBL queries the buffer by calling
[GBL_EFI_BOOT_MEMORY_PROTOCOL.GetBootBuffer()](./gbl_efi_boot_memory_protocol.md#gbl_efi_boot_memory_protocolgetbootbuffer)
with parameter `BootBufferType` set to `PVMFW_DATA`. The buffer is optional. If
not provided, GBL will look for space from the general load buffer.

### Fastboot Download Buffer

GBL uses this buffer as download buffer in fastboot mode.

GBL queries the buffer by calling
[GBL_EFI_BOOT_MEMORY_PROTOCOL.GetBootBuffer()](./gbl_efi_boot_memory_protocol.md#gbl_efi_boot_memory_protocolgetbootbuffer)
with parameter `BootBufferType` set to `FASTBOOT_DOWNLOAD`. If the buffer is not
provided. GBL allocates a default 512MB memory via EFI allocation.

## Partition Read Buffers

By default, GBL allocates memory for reading disk partitions when assembling
boot images. Firmware can override this for individual partitions by providing a
dedicated read buffer via the
[GBL_EFI_BOOT_MEMORY_PROTOCOL.GetPartitionBuffer()](./gbl_efi_boot_memory_protocol.md#gbl_efi_boot_memory_protocolgetpartitionbuffer)
API. Before reading a partition, GBL queries the buffer using the slotless
partition name. For example, before reading `"boot_a/b"` partition, GBL calls
[GBL_EFI_BOOT_MEMORY_PROTOCOL.GetPartitionBuffer()](./gbl_efi_boot_memory_protocol.md#gbl_efi_boot_memory_protocolgetpartitionbuffer)
with parameter `BaseName` set to `"boot"`.

Firmware can also provide preloaded partition data in the buffer. The
`PRELOADED` bit in the output parameter `Flag` should be set in this case. GBL
skips reading the partition when preloaded data is available. This can be used
for optimizing boot performance or overriding target boot images (images still
need to pass AVB verification in locked mode).

If GBL has entered fastboot mode in the same session, preloaded partition data
may be outdated since new images may have been flashed and active slot may have
been changed. In this case, GBL calls
[GBL_EFI_BOOT_MEMORY_PROTOCOL.SyncPartitionBuffer()](./gbl_efi_boot_memory_protocol.md#gbl_efi_boot_memory_protocolsyncpartitionbuffer)
with paramter `SyncPreloaded` set to true after exiting fastboot mode and before
querying any partition buffer. Firmware should either reload the partitions, or
simply clear the `PRELOADED` bit to instruct GBL to read from disk by itself.

After GBL finishes assembling kernel/ramdisk/FDT from partition images, and
before fixing up FDT/bootconfig via
[GBL_EFI_OS_CONFIGURATION_PROTOCOL](./gbl_os_configuration_protocol.md), GBL
calls
[GBL_EFI_BOOT_MEMORY_PROTOCOL.SyncPartitionBuffer()](./gbl_efi_boot_memory_protocol.md#gbl_efi_boot_memory_protocolsyncpartitionbuffer)
with paramter `SyncPreloaded` set to false. Firmware can take this chance to
inspect newly loaded images in the provided partition buffers and determines the
needed FDT/bootconfig fixup if needed.

The following summarizes the order of events discussed above.

1. If GBL previously entered fastboot, calls
   [GBL_EFI_BOOT_MEMORY_PROTOCOL.SyncPartitionBuffer(SyncPreloaded=TRUE)](./gbl_efi_boot_memory_protocol.md#gbl_efi_boot_memory_protocolsyncpartitionbuffer)
2. Queries partition buffer with
   [GBL_EFI_BOOT_MEMORY_PROTOCOL.GetPartitionBuffer()](./gbl_efi_boot_memory_protocol.md#gbl_efi_boot_memory_protocolgetpartitionbuffer).
3. Reads, verifies partition images and assembles kernel/ramdisk/FDT.
4. Calls
   [GBL_EFI_BOOT_MEMORY_PROTOCOL.SyncPartitionBuffer(SyncPreloaded=FALSE)](./gbl_efi_boot_memory_protocol.md#gbl_efi_boot_memory_protocolsyncpartitionbuffer).
5. Fixes up FDT and bootcofnig via
   [GBL_EFI_OS_CONFIGURATION_PROTOCOL](./gbl_os_configuration_protocol.md).
6. Boot.

## AARCH64 Kernel Decopmression

GBL can detect and handle compressed kernel for aarch64. However, current
implementation requires allocating a separate piece of memory for storing
decompressed kernel temporarily. This buffer is allocated via EFI memory
allocation.

## AVB

The AVB (Android Verified Boot) implementation in GBL requires allocating
additional memory for constructing commandline argument strings and loading
vbmeta images from disk and any other vendor required partitions for
verification. The memory is allocated via EFI memory allocation.

### Hash2 Protocol Structures

TODO(b/439659986): embed the protocol structures in AVB context structures.

If the EFI Hash2 protocol is available, GBL uses the Hash2 protocol as part of
AVB verification to calculate image digests. Data structures involved in the
calculation are currently allocated dynamically to avoid problems with structure
definitions between Rust and C. The memory is allocated via EFI memory
allocation.
