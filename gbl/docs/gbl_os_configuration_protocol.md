# GBL OS Configuration EFI Protocol

This document defines an EFI protocol for GBL which allows the device to
apply runtime fixups to data passed into the OS.

## GBL_EFI_OS_CONFIGURATION_PROTOCOL

### Summary

This protocol provides a mechanism for the EFI firmware to build and update OS
configuration data:

* device tree (select components to build the final one)
* kernel commandline (append fixups)
* bootconfig (append fixups)

GBL will load and verify the base data from disk, and then call these protocol
functions to give the firmware a chance to construct and adjust the data as needed
for the particular device. Device tree fixup is handled by `EFI_DT_FIXUP` protocol.

If no runtime modifications are necessary, this protocol may be left
unimplemented.

### GUID

```c
// {dda0d135-aa5b-42ff-85ac-e3ad6efb4619}
#define GBL_EFI_OS_CONFIGURATION_PROTOCOL_GUID       \
  {                                                  \
    0xdda0d135, 0xaa5b, 0x42ff, {                    \
      0x85, 0xac, 0xe3, 0xad, 0x6e, 0xfb, 0x46, 0x19 \
    }                                                \
  }
```

### Revision Number

Note: revision 0 means the protocol is not yet stable and may change in
backwards-incompatible ways.

```c
#define GBL_EFI_OS_CONFIGURATION_PROTOCOL_REVISION 0x00000000
```

### Protocol Interface Structure

```c
typedef struct _GBL_EFI_OS_CONFIGURATION_PROTOCOL {
  UINT64                            Revision;
  GBL_EFI_FIXUP_KERNEL_COMMAND_LINE FixupKernelCommandline;
  GBL_EFI_FIXUP_BOOTCONFIG          FixupBootConfig;
  GBL_EFI_SELECT_DEVICE_TREES       SelectDeviceTrees;
  GBL_EFI_FIXUP_ZBI                 FixupZbi;
} GBL_EFI_OS_CONFIGURATION_PROTOCOL;
```

### Parameters

#### Revision
The revision to which the `GBL_EFI_OS_CONFIGURATION_PROTOCOL` adheres. All
future revisions must be backwards compatible. If a future version is not
backwards compatible, a different GUID must be used.

#### FixupKernelCommandline
Applies kernel commandline fixups. See
[`FixupKernelCommandline()`](#FixupKernelCommandline).

#### FixupBootConfig
Applies bootconfig fixups. See [`FixupBootConfig()`](#FixupBootConfig).

#### SelectDeviceTrees
Select components such as base device tree, overlays to build the final device tree.
See [`SelectDeviceTrees()`](#SelectDeviceTrees).

#### FixupZbi
Applies ZBI fixups (Fuchsia kernels only). See [`FixupZbi()`](#FixupZbi).

## GBL_EFI_OS_CONFIGURATION_PROTOCOL.FixupKernelCommandline() {#FixupKernelCommandline}

### Summary

Provides runtime fixups to the kernel command line.

### Prototype

```c
typedef EFI_STATUS (EFIAPI *GBL_EFI_FIXUP_KERNEL_COMMAND_LINE)(
  IN GBL_EFI_OS_CONFIGURATION_PROTOCOL *This,
  IN CONST CHAR8                       *CommandLine,
  OUT CHAR8                            *Fixup,
  IN OUT UINTN                         *FixupBufferSize
  );
```

### Parameters

Ownership of all the parameters is loaned only for the duration of the function call, and
must not be retained by the protocol after returning.

#### This
A pointer to the `GBL_EFI_OS_CONFIGURATION_PROTOCOL` instance.

#### CommandLine [in]
A pointer to the ASCII nul-terminated command line built by GBL.

#### Fixup [out]
Pointer to a pre-allocated buffer to store the generated command line fixup.
GBL verifies and appends provided data into the final command line. FW may
leave this unchanged if no fixup is required.

The FW implementation can generate a fixup with the following restrictions:
* on return, the data must be valid ASCII encoding with nul termination
* the data and termination byte must never exceed the provided `FixupBufferSize`
* no libavb arguments may be provided (see Security below)

#### FixupBufferSize [in, out]
On function call, this points to the fixup buffer size provided by `Fixup`. The
implementation is free to provide fixup data up to this size, including the
termination byte.

If the buffer is not large enough to fit the fixups, the function should update
`FixupBufferSize` with the required size and return `EFI_BUFFER_TOO_SMALL`;
GBL will then allocate a larger buffer, discard all modifications and repeat
the `FixupKernelCommandline` call.

`FixupBufferSize` does not need to be updated on success, GBL will determine the
fixup command line data size via the nul terminator.

### Description

GBL will call this function after loading and verifying the base kernel command
line, to give the device an opportunity to supply some of the runtime fixups.

Since the device tree selection affects the base kernel command line, GBL will
call `SelectDeviceTrees` first before calling `FixupKernelCommandline`.

#### Security

To ensure the integrity of verified boot data, this protocol will not be
allowed to append any command line parameters provided by
[libavb](https://source.android.com/docs/security/features/verifiedboot/avb).
If any of these parameters are provided, GBL will treat this as a failed boot
attempt:
* `androidboot.veritymode*`
* `androidboot.vbmeta*`
* `dm`
* `root`

Additionally, all data used to apply fixups to the command line must be trusted.
In particular, if the protocol loads any data from non-secure storage, it should
verify that data before use.

#### Status Codes Returned

|                         |                                                                                     |
| ----------------------- | ----------------------------------------------------------------------------------- |
| `EFI_SUCCESS`           | Command line fixup provided.                                                        |
| `EFI_INVALID_PARAMETER` | A parameter is invalid.                                                             |
| `EFI_BUFFER_TOO_SMALL`  | The buffer is too small; `FixupBufferSize` has been updated with the required size. |
| `EFI_DEVICE_ERROR`      | Internal error while providing the command line fixup.                              |

## GBL_EFI_OS_CONFIGURATION_PROTOCOL.FixupBootConfig() {#FixupBootConfig}

### Summary

Provides runtime fixups to the bootconfig.

### Prototype

```c
typedef EFI_STATUS (EFIAPI *GBL_EFI_FIXUP_BOOTCONFIG)(
  IN GBL_EFI_OS_CONFIGURATION_PROTOCOL *This,
  IN CONST CHAR8                       *BootConfig,
  IN UINTN                             BootConfigSize,
  OUT CHAR8                            *Fixup,
  IN OUT UINTN                         *FixupBufferSize
  );
```

### Parameters

Ownership of all the parameters is loaned only for the duration of the function call, and
must not be retained by the protocol after returning.

#### This
A pointer to the `GBL_EFI_OS_CONFIGURATION_PROTOCOL` instance.

#### BootConfig [in]
Pointer to the bootconfig built by GBL. Trailing data isn't provided.

#### BootConfigSize [in]
Size of the bootconfig built by GBL.

#### Fixup [out]
Pointer to a pre-allocated buffer to store the generated bootconfig fixup.
GBL verifies and appends provided data into the final bootconfig. FW may
leave this unchanged if no fixup is required. `FixupBufferSize` must be
updated to `0` in this case.

The FW implementation can generate a fixup with the following restrictions:
* on return, the data must be valid bootconfig (trailer is optional)
* the data must never exceed the provided `FixupBufferSize`
* no libavb arguments may be provided (see Security below)

#### FixupBufferSize [in, out]
On function call, this points to the fixup buffer size provided by `Fixup`. The
implementation is free to provide fixup data up to this size.

If the buffer is not large enough to fit the fixups, the function should update
`FixupBufferSize` with the required size and return `EFI_BUFFER_TOO_SMALL`;
GBL will then allocate a larger buffer, discard all modifications and repeat
the `FixupBootConfig` call.

`FixupBufferSize` must be updated on success to let GBL determine the bootconfig fixup size.

### Description

[Bootconfig](https://source.android.com/docs/core/architecture/bootloader/implementing-bootconfig)
is very similar to the kernel command line, but the format is slightly
different, and the contents are intended for user space consumption rather than
kernel.

This protocol only needs to provide the bootconfig parameters, GBL will automatically update
the bootconfig trailer metadata afterwards.

#### Security

This function's security guidelines are exactly identical to
[`FixupKernelCommandline`](#FixupKernelCommandline); see those docs for details.

#### Status Codes Returned

This function's status return codes are exactly identical to
[`FixupKernelCommandline`](#FixupKernelCommandline); see those docs for details.

## GBL_EFI_OS_CONFIGURATION_PROTOCOL.SelectDeviceTrees() {#SelectDeviceTrees}

### Summary

Inspects device trees and overlays loaded by GBL to determine which ones to use.

### Prototype

```c
typedef enum {
  BOOT,
  VENDOR_BOOT,
  DTBO,
  DTB,
} GBL_EFI_DEVICE_TREE_SOURCE;

typedef enum {
  DEVICE_TREE,
  OVERLAY,
  PVM_DA_OVERLAY,
} GBL_EFI_DEVICE_TREE_TYPE;

typedef struct {
  // GBL_EFI_DEVICE_TREE_SOURCE
  UINT32 Source;
  // GBL_EFI_DEVICE_TREE_TYPE
  UINT32 Type;
  // values are zeroed and must not be used in case of BOOT / VENDOR_BOOT source
  UINT32 Id;
  UINT32 Rev;
  UINT32 Custom[4];
} GBL_EFI_DEVICE_TREE_METADATA;

typedef struct {
  GBL_EFI_DEVICE_TREE_METADATA Metadata;
  // base device tree / overlay buffer (guaranteed to be 8-bytes aligned),
  // cannot be NULL. Device tree size can be identified by the header totalsize field.
  CONST VOID *DeviceTree;
  // Indicates whether this device tree (or overlay) must be included in the
  // final device tree. Set to true by a FW if this component must be used
  BOOLEAN Selected;
} GBL_EFI_VERIFIED_DEVICE_TREE;

typedef EFI_STATUS (EFIAPI *GBL_EFI_SELECT_DEVICE_TREES)(
  IN GBL_EFI_OS_CONFIGURATION_PROTOCOL *This,
  IN OUT GBL_EFI_VERIFIED_DEVICE_TREE  *DeviceTrees,
  IN UINTN                             NumDeviceTrees
  );
```

### Parameters

Ownership of all the parameters is loaned only for the duration of the function call, and
must not be retained by the protocol after returning.

#### This
A pointer to the `GBL_EFI_OS_CONFIGURATION_PROTOCOL` instance.

#### DeviceTrees [in, out]

Pointer to an array containing loaded device tree components along with associated
metadata to distinguish device tree component types (`GBL_EFI_DEVICE_TREE_METADATA.Type`)
and identify source it's loaded from (`GBL_EFI_DEVICE_TREE_METADATA.Source`).

#### NumDeviceTrees [in]

The number of device tree components in the provided `DeviceTrees` array.

### Description

A single set of Android build artifacts may include multiple device tree components
distributed across Android boot partitions such as `boot`, `vendor_boot`, `dtb`,
`dtbo`, etc. It is common practice to leverage this capability to support multiple
SoCs using the same set of artifacts, with the appropriate device tree selected
dynamically at the bootloader stage.

To support this use case, GBL loads all available device tree components and
provides them to the firmware along with associated metadata, enabling selection
via this UEFI call.

Firmware can use the `GBL_EFI_DEVICE_TREE_METADATA.Type` metadata field to
distinguish between different types of device tree components:

1. Device trees (`DEVICE_TREE`) — Base device tree.
2. Device tree overlays (`OVERLAY`) — Overlays to be applied to a base device tree.
3. Device assignment overlays (`PVM_DA_OVERLAY`) — Overlays to be applied to pVMs
   managed by [AVF][avf]. Device assignment overlay is distingueshed from the regular
   host overlay by 31st bit of `dttable` `entry.id`:
   ```
   bool entry_is_vm = (entry.id >> 31) & 0x1;
   ```
   It also affects the exposed `GBL_EFI_DEVICE_TREE_METADATA.Id` value since it's
   a pure copy of coresponding `entry.id`.

The `GBL_EFI_DEVICE_TREE_METADATA.Source` field identifies the origin partition
of each loaded device tree component (`BOOT`, `VENDOR_BOOT`, `DTBO`, `DTB`, `FIT`).

Selection is performed by setting `GBL_EFI_VERIFIED_DEVICE_TREE.Selected` to
`TRUE` on the firmware side, following these rules:

1. Exactly one device tree must be selected. GBL will refuse to boot if none or
   multiple base device trees are selected.
2. Any number of overlays may be selected, including none.
3. At most one pVM device assignment overlay can be selected. If multiple such
   overlays are selected, GBL will refuse to boot.

### Status Codes Returned

|                         |                                                                         |
| ----------------------- | ----------------------------------------------------------------------- |
| `EFI_SUCCESS`           | Base device tree, overlays has been selected.                           |
| `EFI_INVALID_PARAMETER` | A parameter is invalid. For example, incorrect device trees, alignment. |

## GBL_EFI_OS_CONFIGURATION_PROTOCOL.FixupZbi() {#FixupZbi}

TODO(b/353272981)

[avf]:https://source.android.com/docs/core/virtualization
