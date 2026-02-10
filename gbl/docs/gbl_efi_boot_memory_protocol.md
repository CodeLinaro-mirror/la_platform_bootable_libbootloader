# GBL EFI Boot Memory Protocol

|             |                  |
| :---------- | :--------------- |
| **Status**  | Work in progress |
| **Created** | 2025-07-13       |

## GBL_EFI_BOOT_MEMORY_PROTOCOL

### Summary

This document describes the GBL Boot Memory protocol. The protocol allows UEFI
firmware to provde reserved buffers for sharing preloaded partition image to the
bootloader, or as destination buffers for the bootloader to load the images to,
assemble kernel/ramdisk/fdt images, and download data in fastboot mode etc.

### GUID

```c
// {309f2874-ad59-4fd2-af5e-ce0f4ab401a6}
#define GBL_EFI_BOOT_MEMORY_PROTOCOL_GUID            \
  {                                                  \
    0x309f2874, 0xad59, 0x4fd2, {                    \
      0xaf, 0x5e, 0xce, 0x0f, 0x4a, 0xb4, 0x01, 0xa6 \
    }                                                \
  }
```

### Revision Number

```c
#define GBL_EFI_BOOT_MEMORY_PROTOCOL_REVISION GBL_PROTOCOL_REVISION(0, 1)
```

See
[GBL Custom Protocol Revisions](efi_protocols.md#gbl-custom-protocol-revisions)
for details about protocol revisions.

### Protocol Interface Structure

```c
typedef struct _GBL_EFI_BOOT_MEMORY_PROTOCOL {
  UINT64                            Revision;
  GBL_EFI_GET_PARTITION_BUFFER      GetPartitionBuffer;
  GBL_EFI_SYNC_PARTITION_BUFFER     SyncPartitionBuffer;
  GBL_EFI_GET_BOOT_BUFFER           GetBootBuffer;
} GBL_EFI_BOOT_MEMORY_PROTOCOL;
```

### Parameters

**Revision** \
The revision to which the GBL_EFI_BOOT_MEMORY_PROTOCOL adheres. All future
revisions must be backwards compatible. If a future version is not backwards
compatible, a different GUID must be used.

**GetPartitionBuffer** \
Get the reserved memory for loading a specific image. See
[`GBL_EFI_BOOT_MEMORY_PROTOCOL.GetPartitionBuffer()`](#gbl_efi_boot_memory_protocolgetpartitionbuffer).

**SyncPartitionBuffer** \
Notify firmware to inspect or update all reserved buffers for return by
`GetPartitionBuffer()`
[`GBL_EFI_BOOT_MEMORY_PROTOCOL.SyncPartitionBuffer()`](#gbl_efi_boot_memory_protocolsyncpartitionbuffer).

**GetBootBuffer** \
Get the reserved memory for assembling different boot images. See
[`GBL_EFI_BOOT_MEMORY_PROTOCOL.GetBootBuffer()`](#gbl_efi_boot_memory_protocolgetbootbuffer).

## GBL_EFI_BOOT_MEMORY_PROTOCOL.GetPartitionBuffer()

### Summary

`GetPartitionBuffer()` gets the reserved buffer for loading a partition image.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_GET_PARTITION_BUFFER) (
  IN GBL_EFI_BOOT_MEMORY_PROTOCOL *Self,
  IN CONST CHAR                   *BaseName,
  OUT UINTN                       *Size,
  OUT VOID                        **Addr,
  OUT GblEfiPartitionBufferFlag   *Flag,
)
```

### Parameters

**Self** \
A pointer to the [`GBL_EFI_BOOT_MEMORY_PROTOCOL`](#gbl_efi_boot_memory_protocol)
instance.

**BaseName** \
A null-terminated UTF8 encoded string that represents slotless partition name.

**Size** \
On exit, stores the size of the reserved memory in number of bytes.

**Addr** \
On exit, stores the address of the reserved memory.

**Flag** \
On exit, stores a flag that contains additional information for the memory. See
`GblEfiPartitionBufferFlag` for more details.

### Description

The interface is optional and can be used by the firmware to provide designated
buffers for the bootloader to read different images. The firmware can also
preload images to the memory and share it with the bootloader via this
interface. `Flag` should have the `PRELOADED` bit set to 1 in this case. If no
memory is provided, the caller is responsible for finding the needed memory.

Firmware must guarantee that the preloaded data is up-to-date when the API is
called. Each partition image must map to a unique reserved memory that remains
valid for read and write througout the lifetime of the caller app. It's up to
the caller to interpret and validate the content in the memory before use.

Certain partition images have specific alignment requirement in order to be
parsed. For example, for DTB/DTBO images, it is required to be loaded to 8-bytes
aligned buffers. Failing to do so may cause the caller to wrongly process the
image data. Thus firmware should make sure to provide correctly aligned memory
that matches the requirement of the image and caller's expectation. For
alignment requirement on common boot images, see
[`GBL_EFI_BOOT_MEMORY_PROTOCOL.GetBootBuffer()`](#gbl_efi_boot_memory_protocolgetbootbuffer).

Note: each device-specific partition that requests verification in
[GBL_EFI_AVB_PROTOCOL.ReadPartitionsAttributes()](./gbl_efi_avb_protocol.md#gbl_efi_image_loading_protocolreadpartitionsattributes)
will be able to provide a partition buffer using the same partition name.

### Related Definitions

#### GBL_EFI_PARTITION_BUFFER_FLAG

```c
enum {
    GBL_EFI_PARTITION_BUFFER_FLAG_PRELOADED = 1 << 0,
};

typedef uint32_t GBL_EFI_PARTITION_BUFFER_FLAG;
```

##### PRELOADED

If set, it indicates the buffer returned by `GetPartitionBuffer()` already
contains the image loaded by the firwmare.

### Status Codes Returned

| Return Code           | Semantics                                                                          |
| :-------------------- | :--------------------------------------------------------------------------------- |
| EFI_SUCCESS           | Buffer provided successfully                                                       |
| EFI_NOT_FOUND         | The platform does not have reserved memory for this image.                         |
| EFI_INVALID_PARAMETER | `Self` is invalid or any of `ImageType`, `Addr`, `Size` and `IsPreloaded` is NULL. |

## GBL_EFI_BOOT_MEMORY_PROTOCOL.SyncPartitionBuffer()

### Summary

`SyncPartitionBuffer()` notifies the firmware to inspect or update buffers for
return by `GetPartitionBuffer()`.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_SYNC_PARTITION_BUFFER) (
  IN GBL_EFI_BOOT_MEMORY_PROTOCOL *Self,
  IN BOOL                         SyncPreloaded,
)
```

### Parameters

**Self** \
A pointer to the [`GBL_EFI_BOOT_MEMORY_PROTOCOL`](#gbl_efi_boot_memory_protocol)
instance.

**SyncPreloaded** \
Set to true to instruct the firmware to sync preloaded partition data based on
current device state.

### Description

Caller calls this interface to allow the firmware to inspect, update or move the
buffers returned by `GetPartitionBuffer()`. Caller can call this API after
loading new images to the buffers to notify the firmware to process it.

Caller can also call this API with `SyncPreloaded` set to true to request the
firmware to re-sync preloaded partition data. Firmware should either re-load the
partitions if supported, or invalidate existing ones by clearing the `PRELOADED`
bit or returning EFI_NOT_FOUND for future calls of `GetPartitionBuffer()`

For the caller, all buffers previously obtained from `GetPartitionBuffer()`
should not be considered valid anymore.

### Status Codes Returned

| Return Code           | Semantics                   |
| :-------------------- | :-------------------------- |
| EFI_SUCCESS           | Sync completed successfully |
| EFI_DEVICE_ERROR      | An internal error occurred. |
| EFI_INVALID_PARAMETER | `Self` is invalid.          |

### Examples

Below is an example use this API for a bootloader application to interact with
firmware provided partition buffers.

```
GBL_EFI_BOOT_MEMORY_PROTOCOL  *Protocol;
BOOLEAN                       DiskContentChanged;
EFI_STATUS                    Status;

// Makes sure partition buffers is up-to-date
Status = Protocol->SyncPartitionBuffer(DiskContentChanged);
// Interacts with partitions buffers
Status = Protocol->GetPartitionBuffer(Protocol, "boot",...);
...
// Notfiies firmware that new images may have been loaded.
Status = Protocol->SyncPartitionBuffer(FALSE);
...
// Boot OS.
```

## GBL_EFI_BOOT_MEMORY_PROTOCOL.GetBootBuffer()

### Summary

`GetBootBuffer()` get the reserved buffers for bootloader to construct boot
images.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_GET_BOOT_BUFFER) (
  IN GBL_EFI_BOOT_MEMORY_PROTOCOL *Self,
  IN GBL_EFI_BOOT_BUFFER_TYPE     BootBufferType,
  OUT UINTN                       *Size,
  OUT VOID                        **Addr,
)
```

### Parameters

**Self** \
A pointer to the [`GBL_EFI_BOOT_MEMORY_PROTOCOL`](#gbl_efi_boot_memory_protocol)
instance.

**BootBufferType** \
A GBL_EFI_BOOT_BUFFER_TYPE value that identifies the type of boot buffer.

**Size** \
On exit, stores the size of the reserved memory if one is available, or the
recommended size of the memory for the caller to allocate.

**Addr** \
On exit, stores the address of the reserved memory if one is available, or NULL
to indicate that caller can allocate any memory to use.

### Description

The interface can be used by the firmware to provide designated buffers for the
bootloader to assemble different boot images such as kernel, ramdisk, fdt pvmfw
etc, or download data in fastboot.

If no memory is provided, it's up to the caller to decide where to find the
memory needed.

In some cases, the firmware may choose not to reserve a memory for a buffer type
but instead want the caller to allocate it at run time when it is needed. In
this case, `Addr` should be set to NULL and `Size` should be set to the
recommended allocation size. This is useful for buffer types such as fastboot
download buffer which is only needed on demand and can be deallocated when done.

### Related Definitions

#### GBL_EFI_BOOT_BUFFER_TYPE

```c
enum {
  GBL_EFI_BOOT_BUFFER_TYPE_GENERAL_LOAD,
  GBL_EFI_BOOT_BUFFER_TYPE_KERNEL,
  GBL_EFI_BOOT_BUFFER_TYPE_RAMDISK,
  GBL_EFI_BOOT_BUFFER_TYPE_FDT,
  GBL_EFI_BOOT_BUFFER_TYPE_PVMFW_DATA,
  GBL_EFI_BOOT_BUFFER_TYPE_FASTBOOT_DOWNLOAD,
};

typedef uint32_t GBL_EFI_BOOT_BUFFER_TYPE;
```

##### GBL_EFI_BOOT_BUFFER_TYPE_GENERAL_LOAD

General purpose load buffer. This is typically for cases where firmware only
want to provide a single piece of memory for the bootloader to load all of
kernel/ramdisk/fdt and does not care where each one is.

##### GBL_EFI_BOOT_BUFFER_TYPE_KERNEL

Memory for assembling finalized kernel. Must be 2MB aligned.

##### GBL_EFI_BOOT_BUFFER_TYPE_RAMDISK

Memory for assembling finalized ramdisk.

##### GBL_EFI_BOOT_BUFFER_TYPE_FDT

Memory for assembling finalized FDT. Must be 8-bytes aligned.

##### GBL_EFI_BOOT_BUFFER_TYPE_PVM_FW

Memory for loading finalized protected VM firmware. Both the size and address
must be 4K bytes aligned.

##### GBL_EFI_BOOT_BUFFER_TYPE_FASTBOOT_DOWNLOAD

Memory for use as download buffer in fastboot mode.

### Status Codes Returned

| Return Code           | Semantics                                                                                      |
| :-------------------- | :--------------------------------------------------------------------------------------------- |
| EFI_SUCCESS           | Buffer provided successfully                                                                   |
| EFI_NOT_FOUND         | The platform does not have reserved memory, or has no suggested allocation size for this type. |
| EFI_INVALID_PARAMETER | `Self` is invalid or any of `Addr` and `Size` is NULL.                                         |
