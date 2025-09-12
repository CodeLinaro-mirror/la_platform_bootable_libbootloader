# GBL OS Configuration EFI Protocol

This document defines an EFI protocol for GBL which allows the device to
apply runtime fixups to data passed into the OS.

## GBL_EFI_OS_CONFIGURATION_PROTOCOL

### Summary

This protocol provides a mechanism for the EFI firmware to build and update OS
configuration data:

* device tree (select components to build the final one)
* bootconfig (append fixups)
* FIT configuration (select configuration corresponding to the platform)

GBL will load and verify the data provided by boot partitions, and then call
these protocol functions to give the firmware a chance to construct and adjust
the data as needed for the particular device. Device tree fixup (including
kernel command line) is handled by `DT_FIXUP` protocol.

If no runtime modifications are necessary, this protocol may be left
unimplemented, so GBL autoselection logic will be used. Refer to
[`SelectDeviceTrees()`][select] description for more details.

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

```c
#define GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL_REVISION GBL_PROTOCOL_REVISION(0, 1)
```

See [GBL Custom Protocol Revisions](efi_protocols.md#gbl-custom-protocol-revisions) for details about protocol revisions.

### Protocol Interface Structure

```c
typedef struct _GBL_EFI_OS_CONFIGURATION_PROTOCOL {
  UINT64                            Revision;
  GBL_EFI_FIXUP_BOOTCONFIG          FixupBootConfig;
  GBL_EFI_SELECT_DEVICE_TREES       SelectDeviceTrees;
  GBL_EFI_SELECT_FIT_CONFIGURATION  SelectFitConfiguration;
  GBL_EFI_FIXUP_ZBI                 FixupZbi;
} GBL_EFI_OS_CONFIGURATION_PROTOCOL;
```

### Parameters

#### Revision

The revision to which the `GBL_EFI_OS_CONFIGURATION_PROTOCOL` adheres. All
future revisions must be backwards compatible. If a future version is not
backwards compatible, a different GUID must be used.

#### FixupBootConfig

Applies bootconfig fixups. See [`FixupBootConfig()`][bootconfig_fixup].

#### SelectDeviceTrees

Select components such as base device tree, overlays to build the final device
tree. See [`SelectDeviceTrees()`][select].

#### SelectFitConfiguration

Selects the FIT configuration corresponding to the platform.
See [`SelectFitConfiguration()`][fit_configuration]

#### FixupZbi

Applies ZBI fixups (Fuchsia kernels only). See [`FixupZbi()`][fixup_zbi].

## GBL_EFI_OS_CONFIGURATION_PROTOCOL.FixupBootConfig()

### Summary

Provides runtime fixups to the bootconfig.

### Prototype

```c
typedef EFI_STATUS (EFIAPI *GBL_EFI_FIXUP_BOOTCONFIG)(
  IN GBL_EFI_OS_CONFIGURATION_PROTOCOL *Self,
  IN CONST CHAR8                       *BootConfig,
  IN UINTN                             BootConfigSize,
  OUT CHAR8                            *Fixup,
  IN OUT UINTN                         *FixupBufferSize
  );
```

### Parameters

Ownership of all the parameters is loaned only for the duration of the function
call, and must not be retained by the protocol after returning.

#### Self

A pointer to the `GBL_EFI_OS_CONFIGURATION_PROTOCOL` instance.

#### BootConfig [in]

Pointer to the bootconfig built by GBL. Trailing data isn't provided.

#### BootConfigSize [in]

Size of the bootconfig built by GBL.

#### Fixup [out]

Pointer to a pre-allocated buffer to store the generated bootconfig fixup. GBL
verifies and appends provided data into the final bootconfig. FW may either
return `EFI_UNSUPPORTED`, or leave the buffer unchanged and set
`FixupBufferSize` to `0` to indicate that no fixup is required.

The FW implementation can generate a fixup with the following restrictions:

* on return, the data must be valid bootconfig (trailer is optional)
* provided data must never exceed the provided `FixupBufferSize`
* no libavb arguments may be provided (see Security below)

#### FixupBufferSize [in, out]

On function call, this points to the fixup buffer size provided by `Fixup`. The
implementation is free to provide fixup data up to this size.

If the buffer is not large enough to fit the fixup, implementation must update
`FixupBufferSize` with the required size and return `EFI_BUFFER_TOO_SMALL`; GBL
will then allocate a larger buffer, discard all modifications and repeat the
`FixupBootConfig` call.

`FixupBufferSize` must be updated on success to let GBL determine the provided
bootconfig fixup size.

### Description

[Bootconfig][bootconfig] as a format is similar to the kernel command line, but
intended for user space consumption rather than kernel.

Implementation should only append the bootconfig parameters, GBL will
automatically update the bootconfig trailer metadata afterwards. Override
bootconfig operator `:=` may be used to re-define some of the values provided by
GBL.

#### Security

To ensure the integrity of verified boot data, this protocol will not be
allowed to append any bootconfig provided by [libavb][libavb]. If any of these
parameters are provided, GBL will treat this as a failed boot attempt:

* `androidboot.veritymode*`
* `androidboot.vbmeta*`
* `:=` may be only used to re-define `androidboot.mode`

Additionally, all data used to apply fixups to the bootconfig must be trusted.
In particular, if the protocol loads any data from non-secure storage, it must
verify that data before use.

#### Status Codes Returned

|                         |                                                                                         |
| ----------------------- | --------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`           | Bootconfig fixup provided.                                                              |
| `EFI_UNSUPPORTED`       | No fixup is provided; the bootconfig generated by GBL will be used as-is.               |
| `EFI_BUFFER_TOO_SMALL`  | `Fixup` buffer is too small; `FixupBufferSize` has been updated with the required size. |
| `EFI_INVALID_PARAMETER` | Unexpected input; GBL will refuse to boot.                                                |

## GBL_EFI_OS_CONFIGURATION_PROTOCOL.SelectDeviceTrees()

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
  IN GBL_EFI_OS_CONFIGURATION_PROTOCOL *Self,
  IN OUT GBL_EFI_VERIFIED_DEVICE_TREE  *DeviceTrees,
  IN UINTN                             NumDeviceTrees
  );
```

### Parameters

Ownership of all the parameters is loaned only for the duration of the function
call, and must not be retained by the protocol after returning.

#### Self

A pointer to the `GBL_EFI_OS_CONFIGURATION_PROTOCOL` instance.

#### DeviceTrees [in, out]

Pointer to an array containing loaded device tree components along with
associated metadata to distinguish device tree component types
(`GBL_EFI_DEVICE_TREE_METADATA.Type`) and identify source it's loaded from
(`GBL_EFI_DEVICE_TREE_METADATA.Source`).

#### NumDeviceTrees [in]

The number of device tree components in the provided `DeviceTrees` array.

### Description

A single set of Android build artifacts may include multiple device tree
components distributed across Android boot partitions such as `boot`,
`vendor_boot`, `dtb`, `dtbo`, etc. It is common practice to leverage this
capability to support multiple SoCs using the same set of artifacts, with the
appropriate device tree selected dynamically at the bootloader stage.

To support this use case, GBL loads all available device tree components and
provides them to the firmware along with associated metadata, enabling selection
via this UEFI call.

Firmware can use the `GBL_EFI_DEVICE_TREE_METADATA.Type` metadata field to
distinguish between different types of device tree components:

1. Device trees (`DEVICE_TREE`) — Base device tree.
2. Device tree overlays (`OVERLAY`) — Overlays to be applied to a base device
   tree.
3. Device assignment overlays (`PVM_DA_OVERLAY`) — Overlays to be applied to
   pVMs managed by [AVF][avf]. Device assignment overlay is distingueshed from
   the regular host overlay by 31st bit of `dttable` `entry.id`:

   ```c
   bool entry_is_vm = (entry.id >> 31) & 0x1;
   ```

   It also affects the exposed `GBL_EFI_DEVICE_TREE_METADATA.Id` value since
   it's a pure copy of coresponding `entry.id`.

The `GBL_EFI_DEVICE_TREE_METADATA.Source` field identifies the origin partition
of each loaded device tree component (`BOOT`, `VENDOR_BOOT`, `DTBO`, `DTB`,
`FIT`).

Selection is performed by setting `GBL_EFI_VERIFIED_DEVICE_TREE.Selected` to
`TRUE` on the firmware side, following these rules:

1. Exactly one device tree must be selected. GBL will refuse to boot if none or
   multiple base device trees are selected.
2. Overlays are guaranteed to be provided in the same order as they appear in
   the `dtbo` partition and the selected ones are applied in that same order.
   Any number of overlays may be selected, including none.
3. At most one pVM device assignment overlay can be selected. If multiple such
   overlays are selected, GBL will refuse to boot.

`EFI_UNSUPPORTED` may be returned to indicate that firmware-specific selection
isn't required. GBL will use its default autoselection logic, which selects the
single provided base device tree without applying any overlays. GBL will fail to
boot if more than one base device tree is provided by the boot partitions.

### Status Codes Returned

|                         |                                                                         |
| ----------------------- | ----------------------------------------------------------------------- |
| `EFI_SUCCESS`           | Base device tree, overlays, DA overlays have been selected.             |
| `EFI_UNSUPPORTED`       | No components been selected; GBL will use autoselection.                |
| `EFI_INVALID_PARAMETER` | Unexpected input; GBL will refuse to boot.                              |

## GBL_EFI_OS_CONFIGURATION_PROTOCOL.SelectFitConfiguration()

### Summary

Inspects FIT configurations and selects the configuration to be used for
the platform.

### Prototype

```c
typedef EFI_STATUS (EFIAPI *GBL_EFI_SELECT_FIT_CONFIGURATION)(
  IN GBL_EFI_OS_CONFIGURATION_PROTOCOL *This,
  IN UINTN                             FitSize,
  IN CONST UINT8                       *Fit,
  IN UINTN                             MetadataSize,
  IN CONST UINT8                       *Metadata,
  OUT UINTN                            *SelectedConfigurationOffset,
  );
```

### Parameters

Ownership of all the parameters is loaned only for the duration of the function
call, and must not be retained by the protocol after returning.

#### This

A pointer to the `GBL_EFI_OS_CONFIGURATION_PROTOCOL` instance.

#### FitSize [in]

Size of the FIT FDT buffer.

#### Fit [in]

Pointer to the FIT FDT loaded by GBL.

#### MetadataSize [in]

Size of the metadata payload.
The size is guaranteed to be `0` if `Metadata` is NULL.

#### Metadata [in]

Pointer to the first FIT image payload if the type is set to "metadata".

GBL requires the metadata payload to be referenced by the first sub-node
inside the `/images` node in FIT FDT. The sub-node for metadata must have type
set to "metadata".

If no such metadata node is found, this parameter will have a `NULL` value.

#### SelectedConfigurationOffset [out]

Pointer to a value to be set by the firmware with the offset of the
selected configuration node within the `Fit` FDT.

### Description

A single FIT image can contain multiple configurations with various images -
kernel, DTB, ramdisk, etc. Each configuration can have different set of images
corresponding to the platform on which images need to be loaded.
Present GBL implementation supports only devicetree selection via FIT image.

The appropriate configuration can be dynamically selected by the firmware at
runtime based on the platform. Firmware can use the `Metadata` parameter to
read additional information required for comparing the configuration entries
present in the FIT image.

Exactly one FIT configuration must be selected. GBL will refuse to boot if no
configuration is selected.
`EFI_UNSUPPORTED` may be returned to indicate that firmware-specific selection
isn't required. In this case, GBL will use traditional device tree selection
and ignore the FIT image entirely.

### Status Codes Returned

|                         |                                                    |
| ----------------------- | ---------------------------------------------------|
| `EFI_SUCCESS`           | FIT configuration has been selected.               |
| `EFI_UNSUPPORTED`       | No configuration selected; GBL will continue to    |
|                         | boot.                                              |
| `EFI_INVALID_PARAMETER` | Unexpected input; GBL will refuse to boot.         |

## GBL_EFI_OS_CONFIGURATION_PROTOCOL.FixupZbi()

TODO(b/353272981)

[select]: #gbl_efi_os_configuration_protocolselectdevicetrees
[bootconfig_fixup]: #gbl_efi_os_configuration_protocolfixupbootconfig
[fixup_zbi]: #gbl_efi_os_configuration_protocolfixupzbi
[avf]: https://source.android.com/docs/core/virtualization
[bootconfig]: https://source.android.com/docs/core/architecture/bootloader/implementing-bootconfig
[libavb]: https://source.android.com/docs/security/features/verifiedboot/avb
[fit_configuration]: #gbl_efi_os_configuration_protocolselectfitconfiguration
