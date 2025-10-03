# GBL EFI Android Verified Boot Protocol

|||
| :--- | :--- |
| **Status** | Work in progress |
| **Created** | 2024-11-15 |

## GBL_EFI_AVB_PROTOCOL

### Summary

Android Verified Boot ([AVB][avb]) is a process of assuring the end user of the
integrity of the software running on a device. This protocol allows
vendor-specific [AVB][avb] logic to be implemented by the firmware, enabling
device-specific security mechanisms to ensure the integrity of the HLOS.

The `GBL_EFI_AVB_PROTOCOL` is not required for the development GBL flavor,
which is intended to support basic Android boot functionality on unlocked
development boards. However, this protocol must be implemented on production
devices.

### GUID

```c
// {6bc66b9a-d5c9-4c02-9da9-50af198d912c}
#define GBL_EFI_AVB_PROTOCOL_UUID                    \
  {                                                  \
    0x6bc66b9a, 0xd5c9, 0x4c02, {                    \
      0x9d, 0xa9, 0x50, 0xaf, 0x19, 0x8d, 0x91, 0x2c \
    }                                                \
  }
```

### Revision Number

```c
#define GBL_EFI_AVB_PROTOCOL_REVISION GBL_PROTOCOL_REVISION(0, 3)
```

See [GBL Custom Protocol Revisions](efi_protocols.md#gbl-custom-protocol-revisions) for details about protocol revisions.

### Protocol Interface Structure

```c
typedef struct _GBL_EFI_AVB_PROTOCOL {
  UINT64 Revision;
  GBL_EFI_AVB_READ_PARTITIONS_TO_VERIFY ReadPartitionsToVerify;
  GBL_EFI_AVB_READ_DEVICE_STATUS ReadDeviceStatus;
  GBL_EFI_AVB_VALIDATE_VBMETA_PUBLIC_KEY ValidateVbmetaPublicKey;
  GBL_EFI_AVB_READ_ROLLBACK_INDEX ReadRollbackIndex;
  GBL_EFI_AVB_WRITE_ROLLBACK_INDEX WriteRollbackIndex;
  GBL_EFI_AVB_READ_PERSISTENT_VALUE ReadPersistentValue;
  GBL_EFI_AVB_WRITE_PERSISTENT_VALUE WritePersistentValue;
  GBL_EFI_AVB_HANDLE_VERIFICATION_RESULT HandleVerificationResult;
} GBL_EFI_AVB_PROTOCOL;
```

### Parameters

#### Revision

The revision to which the `GBL_EFI_AVB_PROTOCOL` adheres. All future revisions
must be backwards compatible. If a future version is not backwards compatible,
a different GUID must be used.

#### ReadPartitionsToVerify

Retrieves the list of additional partitions to be verified, beyond the standard
set loaded and verified by GBL.
[`ReadPartitionsToVerify()`][readpartitionstoverify].

#### ReadDeviceStatus

Retrieves the current device status, including its lock state and dm-verity
error indication.
[`ReadDeviceStatus()`][readdevicestatus].

#### ValidateVbmetaPublicKey

Validates proper public key is used to sign HLOS artifacts.
[`ValidateVbmetaPublicKey()`](#gbl_efi_avb_protocolvalidatevbmetapublickey).

#### ReadRollbackIndex

Retrieves the rollback index corresponding to the provided index location.
[`ReadRollbackIndex()`](#gbl_efi_avb_protocolreadrollbackindex).

#### WriteRollbackIndex

Writes the rollback index corresponding to the provided index location.
[`WriteRollbackIndex()`][protocolwriterollbackindex].

#### ReadPersistentValue

Retrieves the persistent value for the provided name.
[`ReadPersistentValue()`](#gbl_efi_avb_protocolreadpersistentvalue).

#### WritePersistentValue

Writes or clears the persistent value for the provided name.
[`WritePersistentValue()`](#gbl_efi_avb_protocolwritepersistentvalue).

#### HandleVerificationResult

Handles the AVB verification result (e.g., updating the Root of Trust, setting
device state, displaying UI warnings/errors, handling anti-tampering, etc.).
[`HandleVerificationResult()`][handleverificationresult].

## GBL_EFI_AVB_PROTOCOL.ReadPartitionsToVerify()

### Summary

Retrieves the list of additional partitions to be verified, beyond the standard
set loaded and verified by GBL.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_READ_PARTITIONS_TO_VERIFY) (
  IN GBL_EFI_AVB_PROTOCOL *Self,
  IN OUT UINTN *NumPartitions,
  IN OUT GBL_EFI_AVB_PARTITION *Partitions);
```

### Related Definitions

#### GBL_EFI_AVB_PARTITION_FLAGS

```c
typedef UINT64 GBL_EFI_AVB_PARTITION_FLAGS;
STATIC CONST GBL_EFI_AVB_PARTITION_FLAGS GBL_EFI_AVB_PARTITION_OPTIONAL = 0x1 << 0;
```

##### GBL_EFI_AVB_PARTITION_OPTIONAL

Flag indicating the partition is optional for loading/verification. This allows
firmware to handle its absence at runtime instead of triggering a GBL failure.
If set, GBL can boot even if the partition is missing or not referenced by the
`vbmeta` header. Otherwise, the partition is required, and its absence causes
GBL boot failure in locked mode.

#### GBL_EFI_AVB_PARTITION

```c
typedef
struct GblEfiAvbPartition {
  UINTN BaseNameLen;
  UINT8* BaseName;
  GBL_EFI_AVB_PARTITION_FLAGS Flags;
} GBL_EFI_AVB_PARTITION;
```

##### BaseNameLen

On input, specifies the size of the buffer pointed to by `BaseName`. The
firmware is expected to fill this buffer with the UTF-8 slotless partition name
(e.g., `boot` for `boot_a`). On output, this value must be updated to reflect
the number of bytes copied into the buffer pointed by `BaseName`.

##### BaseName

A pointer to a buffer of `BaseNameLen` bytes available for the implementation to
copy the UTF-8 slotless partition name (e.g `boot` for `boot_a`). A null
terminator is not required to be included.

### Parameters

#### Self

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### NumPartitions

Number of `Partitions` available to be filled by the FW. Must be updated to the
number of partitions returned. If there are no extra partitions to be verified,
`NumPartitions` must be set to 0.

#### Partitions

Pointer to an array of [`GBL_EFI_AVB_PARTITION`](#gbl_efi_avb_partition) with
`NumPartitions` elements, to be filled by the FW with additional partitions that
GBL will load and verify.

#### Flags

A bitmask of flags providing additional metadata for a partition request. See
[`GBL_EFI_AVB_PARTITION_FLAGS`](#gbl_efi_avb_partition_flags) for details.

### Description

GBL loads and verifies a default set of partitions required to boot the HLOS.
For example, in case of Android, GBL loads and verifies the following standard
set of partitions: `boot`, `init_boot`, `vendor_boot`, `vendor_kernel_boot`,
`dtb`, `dtbo`, and `pvmfw`, which are used to boot the system.

This method allows the firmware to specify extra non-standard partitions that
GBL will also load and verify, both to extend the integrity check and to enable
the firmware to handle device-specific partitions content via
[`HandleVerificationResult()`][handleverificationresult] once verification is
complete.

For example, to provide N additional partitions, firmware must update the
`NumPartitions` to N and fill first N elements of `Partitions` following the
[`GBL_EFI_AVB_PARTITION`](#gbl_efi_avb_partition) format. If no extra partitions
are requested to be verified, `NumPartitions` must be set to 0 or
`EFI_UNSUPPORTED` is returned.

If a requested partition (excluding those marked as optional) doesn't exist or
lacks a corresponding hash descriptor in `vbmeta` or a chained partition, it
cannot be verified. GBL will handle this case as follows:

1. For a locked device: `RED` boot status color, so fail to boot.
2. For an unlocked device: `ORANGE` boot status color, still can boot.

### Status Codes Returned

|||
| --- | --- |
| `EFI_SUCCESS` | Successfully provided additional partitions to verify |
| `EFI_UNSUPPORTED` | No extra partitions need to be verified |
| `EFI_BUFFER_TOO_SMALL` | Provided list of `Partitions` is too small; `NumPartitions` has been updated with the required amount. GBL will call this method again with extended `Partitions`. |
| `EFI_BAD_BUFFER_SIZE` | One of provided `Partition.NameLen` values is not sufficient to hold the partition name. GBL will fail to boot. |

## GBL_EFI_AVB_PROTOCOL.ReadDeviceStatus()

### Summary

Allows the firmware to provide current device status, including its lock state
and dm-verity error indication in a vendor-specific way.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_READ_DEVICE_STATUS) (
  IN GBL_EFI_AVB_PROTOCOL *Self,
  OUT GBL_EFI_AVB_DEVICE_STATUS *StatusFlags);
```

### Related Definitions

#### GBL_EFI_AVB_DEVICE_STATUS

```c
typedef UINT64 GBL_EFI_AVB_DEVICE_STATUS;

STATIC CONST GBL_EFI_AVB_DEVICE_STATUS GBL_EFI_AVB_DEVICE_STATUS_UNLOCKED = 0x1 << 0;
STATIC CONST GBL_EFI_AVB_DEVICE_STATUS GBL_EFI_AVB_DEVICE_STATUS_DM_VERITY_FAILED = 0x1 << 1;
```

##### GBL_EFI_AVB_DEVICE_STATUS_UNLOCKED

Flag indicating that the device is unlocked.

##### GBL_EFI_AVB_DEVICE_STATUS_DM_VERITY_FAILED

Flag indicating that the device rebooted due to a dm-verity error.

### Parameters

#### Self

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### StatusFlags

An output parameter to be updated by firmware with ORed flags detailing the AVB
device status. All bits not explicitly defined must be set to zero. See related
definitions above for the semantics of each flag value.

### Description

This method allows the firmware to provide GBL with the current AVB device
status, covering:

1. `GBL_EFI_AVB_DEVICE_STATUS_UNLOCKED` - Indicates the device is
   [unlocked][unlocked]. GBL treats unlocked devices as being in the `orange`
   boot state, skipping certain verification enforcement and allowing boot to
   proceed with reduced security guarantees.
1. `GBL_EFI_AVB_DEVICE_STATUS_DM_VERITY_FAILED` - Indicates the device rebooted
   due to a dm-verity hashtree corruption [error][dmv_error]. In this case, GBL
   passes `AVB_SLOT_VERIFY_FLAGS_RESTART_CAUSED_BY_HASHTREE_CORRUPTION` to
   `libavb`. Unless the library detects new OS images, this results in a
   `GBL_EFI_AVB_BOOT_COLOR_RED_EIO` flag, requiring user additional confirmation
   before proceeding in degraded mode.

GBL may call this method multiple times within a single boot session. If the
method returns an error, GBL rejects to boot.

### Status Codes Returned

|||
| --- | --- |
| `EFI_SUCCESS` | A device status is successfully returned. |
| `EFI_INVALID_PARAMETER` | Unexpected arguments combination. GBL rejects to boot. |

## GBL_EFI_AVB_PROTOCOL.ValidateVbmetaPublicKey()

### Summary

Allows the firmware to verify the public key used to sign the `vbmeta` partition
in a vendor-specific way.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_VALIDATE_VBMETA_PUBLIC_KEY) (
  IN GBL_EFI_AVB_PROTOCOL *Self,
  IN UINTN PublicKeyLength,
  IN CONST UINT8 *PublicKeyData,
  IN UINTN PublicKeyMetadataLength,
  IN CONST UINT8 *PublicKeyMetadata,
  /* GBL_EFI_AVB_KEY_VALIDATION_STATUS */ OUT UINT32 *ValidationStatus);
```

### Related Definitions

#### GBL_EFI_AVB_KEY_VALIDATION_STATUS

```c
// Vbmeta key validation status.
//
// https://source.android.com/docs/security/features/verifiedboot/boot-flow#locked-devices-with-custom-root-of-trust
typedef enum {
  GBL_EFI_AVB_KEY_INVALID,
  GBL_EFI_AVB_KEY_VALID_CUSTOM_KEY,
  GBL_EFI_AVB_KEY_VALID,
} GBL_EFI_AVB_KEY_VALIDATION_STATUS;
```

##### GBL_EFI_AVB_KEY_INVALID

The public key is not valid. The device cannot continue the boot process for
locked devices; GBL reports a `RED` status and resets. Unlocked devices can
still boot with an `ORANGE` state.

##### GBL_EFI_AVB_KEY_VALID_CUSTOM_KEY

The public key is valid but not fully trusted. GBL continues booting a locked
device with a `YELLOW` state and an unlocked device with an `ORANGE` state.

##### GBL_EFI_AVB_KEY_VALID

The public key is valid and trusted, so the device can continue the boot process
for both locked and unlocked states.

### Parameters

#### Self

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### PublicKeyLength

Specifies the length of the public key provided by `PublicKeyData`.

#### PublicKeyData

A pointer to the public key extracted from `vbmeta`. Guaranteed to contain valid
data of length `PublicKeyLength`.

#### PublicKeyMetadataLength

Specifies the length of the public key metadata provided by `PublicKeyMetadata`.
Guaranteed to be 0 in case of `NULL` `PublicKeyMetadata`.

#### PublicKeyMetadata

A pointer to public key metadata provided using the `--public_key_metadata`
`avbtool`'s flag. May be `NULL` if no public key metadata is provided.

#### ValidationStatus

An output parameter that communicates the verification status to GBL. `VALID`
and `VALID_CUSTOM_KEY` are interpreted as successful validation statuses.

### Description

This method allows FW to perform device-specific validation of the public key
extracted from the `vbmeta` partition. This typically involves checking the
provided key against a hardware-trusted root of trust or a pre-provisioned key
stored in secure firmware.

`ValidateVbmetaPublicKey` must set `ValidationStatus` and return `EFI_SUCCESS`.
Any return value other than `EFI_SUCCESS` is treated as a fatal verification
error, resulting in a `RED` state being reported and GBL failing to boot, even
if the device is unlocked.

GBL calls this function once per AVB verification session.

### Status Codes Returned

|||
| --- | --- |
| `EFI_SUCCESS` | Public key validation was successfully completed. |
| `EFI_INVALID_PARAMETER` | Unexpected arguments combination. GBL rejects to boot. |

## GBL_EFI_AVB_PROTOCOL.ReadRollbackIndex()

### Summary

Allows the firmware to provide rollback index for the provided index location to
GBL in a vendor-specific way.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_READ_ROLLBACK_INDEX) (
  IN GBL_EFI_AVB_PROTOCOL *Self,
  IN UINTN IndexLocation,
  OUT UINT64 *RollbackIndex);
```

### Parameters

#### Self

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### IndexLocation

The location of the rollback index to be provided by this method.

#### RollbackIndex

An output parameter used to return the rollback index corresponding to the
provided `IndexLocation`.

### Description

GBL requests rollback indexes to compare against the value provided in the
vbmeta header. This prevents a locked device from booting if the rollback index
provided by the partition is smaller than the value previously written using
[`WriteRollbackIndex`][protocolwriterollbackindex] during the last successful
boot ensuring [rollback protection][rp] in case of an OTA.

GBL only requests rollback indexes for `IndexLocation` equals `0` as a global
HLOS index or locations specified in the corresponding chained partition
descriptors. Returning any error in such cases causes GBL boot failure for
locked devices.

### Status Codes Returned

|||
| --- | --- |
| `EFI_SUCCESS` | The rollback index value is successfully returned. |
| `EFI_NOT_FOUND` | The requested rollback index isn't supported, so cannot be returned. GBL rejects to boot. |
| `EFI_INVALID_PARAMETER` | Unexpected arguments combination. GBL rejects to boot. |

## GBL_EFI_AVB_PROTOCOL.WriteRollbackIndex()

### Summary

Allows the firmware to update rollback index for the provided index location in
a vendor-specific way.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_WRITE_ROLLBACK_INDEX) (
  IN GBL_EFI_AVB_PROTOCOL *Self,
  IN UINTN IndexLocation,
  IN UINT64 RollbackIndex);
```

### Parameters

#### Self

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### IndexLocation

The location of the rollback index to be set by this method.

#### RollbackIndex

A rollback index value to be set for the provided `IndexLocation`.

### Description

For a locked device, if a known-good slot is successfully verified, GBL updates
rollback indexes to the value provided in the vbmeta header in accordance with
`libavb` [requirements][update_ri]. This prevents a locked device from booting a
previous version of HLOS on the next boot, ensuring [rollback protection][rp] in
case of an OTA.

GBL only updates rollback indexes for `IndexLocation` equals `0` as a global
HLOS index or locations specified in the corresponding chained partition
descriptors. Returning any error in such cases causes GBL boot failure for
locked devices.

### Status Codes Returned

|||
| --- | --- |
| `EFI_SUCCESS` | The rollback index value is successfully updated. |
| `EFI_NOT_FOUND` | The requested rollback index isn't supported, so cannot be updated. GBL rejects to boot. |
| `EFI_INVALID_PARAMETER` | Unexpected arguments combination. GBL rejects to boot. |

## GBL_EFI_AVB_PROTOCOL.ReadPersistentValue()

### Summary

Allows the firmware to read a persistent value associated with the given name in
a vendor-specific manner.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_READ_PERSISTENT_VALUE) (
  IN GBL_EFI_AVB_PROTOCOL *Self,
  IN CONST CHAR8 *Name,
  IN OUT UINTN *ValueSize,
  OUT UINT8 *Value);
```

### Parameters

#### Self

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### Name

Points to null-terminated UTF-8 name for the requested persistent value.

#### ValueSize

On input, points to the size of the provided `Value` buffer, or `0` if GBL only
wants to check the value's availability. On output, the firmware should update
it to reflect the actual size of the value.

#### Value

Points to the buffer of `ValueSize` bytes to be filled by FW with a requested
value. May be `NULL` if GBL only wants to check value's availability.

### Description

GBL requests `avb.persistent_digest.<partition_name>` persistent values to
support the [persistent digest][pd] `libavb` feature. Additionally, GBL may
request the `avb.managed_verity_mode` persistent value to detect HLOS updates to
handle [dm-verity][dmv_error] errors and EIO mode.

### Status Codes Returned

|||
| --- | --- |
| `EFI_SUCCESS` | The requested persistent value is presented and successfully provided in case `Value` buffer isn't NULL. |
| `EFI_NOT_FOUND` | The requested persistent value is not yet populated or supported. GBL will try to initialize it using `WritePersistentValue`. |
| `EFI_BUFFER_TOO_SMALL` | The provided `Value` buffer is too small. GBL rejects to boot. |
| `EFI_INVALID_PARAMETER` | Unexpected arguments combination. GBL rejects to boot. |

## GBL_EFI_AVB_PROTOCOL.WritePersistentValue()

### Summary

Allows the firmware to write a persistent value for the provided name in a
vendor-specific way.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_WRITE_PERSISTENT_VALUE) (
  IN GBL_EFI_AVB_PROTOCOL *Self,
  IN CONST CHAR8 *Name,
  IN UINTN ValueSize,
  IN CONST UINT8 *Value);
```

### Parameters

#### Self

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### Name

Points to null-terminated UTF-8 name for the persistent value to update.

#### ValueSize

Points to the size of the `Value` to be set. May be `0`, in which case the
corresponding persistent value must be treated as not present after such
operation.

#### Value

Points to a buffer of `ValueSize` bytes containing the value to set.

### Description

GBL initializes `avb.persistent_digest.<partition_name>` persistent values to
support the [persistent digest][pd] `libavb` feature. Additionally, if a
[dm-verity][dmv_error] error occurs, GBL updates the `avb.managed_verity_mode`
persistent value with the current vbmeta digest. This allows detection of HLOS
updates in order to disable EIO mode.

### Status Codes Returned

|||
| --- | --- |
| `EFI_SUCCESS` | The value for `Name` is successfully updated. |
| `EFI_NOT_FOUND` | Updating the value for `Name` isn't supported. GBL rejects to boot. |
| `EFI_INVALID_PARAMETER` | The `ValueSize` is too big or any other unexpected arguments combination. GBL rejects to boot. |

## GBL_EFI_AVB_PROTOCOL.HandleVerificationResult()

### Summary

Allows the firmware to handle the verification result in a vendor-specific
manner.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_HANDLE_VERIFICATION_RESULT) (
  IN GBL_EFI_AVB_PROTOCOL *Self,
  IN CONST GBL_EFI_AVB_VERIFICATION_RESULT *Result);
```

### Related Definitions

#### GBL_EFI_AVB_BOOT_COLOR_FLAGS

```c
typedef UINT64 GBL_EFI_AVB_BOOT_COLOR_FLAGS;

STATIC CONST GBL_EFI_AVB_BOOT_COLOR_FLAGS GBL_EFI_AVB_BOOT_COLOR_RED = 0x1 << 0;
STATIC CONST GBL_EFI_AVB_BOOT_COLOR_FLAGS GBL_EFI_AVB_BOOT_COLOR_ORANGE = 0x1 << 1;
STATIC CONST GBL_EFI_AVB_BOOT_COLOR_FLAGS GBL_EFI_AVB_BOOT_COLOR_YELLOW = 0x1 << 2;
STATIC CONST GBL_EFI_AVB_BOOT_COLOR_FLAGS GBL_EFI_AVB_BOOT_COLOR_GREEN = 0x1 << 3;
STATIC CONST GBL_EFI_AVB_BOOT_COLOR_FLAGS GBL_EFI_AVB_BOOT_COLOR_RED_EIO = 0x1 << 4;
```

##### GBL_EFI_AVB_BOOT_COLOR_RED

Flag indicating a verification failure, including fatal errors on unlocked
devices and missing required partitions on locked devices. A corresponding
notification [must][boot_flow_red] be shown to inform the user that no valid OS
was detected. Boot cannot proceed.

##### GBL_EFI_AVB_BOOT_COLOR_ORANGE

Flag indicating that the device is unlocked (regardless of the verification
result). A corresponding notification [must][boot_flow_orange] be shown to
obtain user confirmation before proceeding with the boot. HLOS functionality may
be limited.

##### GBL_EFI_AVB_BOOT_COLOR_YELLOW

Flag indicating that device is locked and verification passed using a
user-provided custom key. A corresponding notification [must][boot_flow_yellow]
be shown to obtain user confirmation before proceeding with the boot.

##### GBL_EFI_AVB_BOOT_COLOR_GREEN

Flag indicating that device is locked and verification passed. Boot can proceed.

##### GBL_EFI_AVB_BOOT_COLOR_RED_EIO

Flag indicating the device has rebooted due to [dm-verity][dmv_error] hash tree
corruption (detected via [`ReadDeviceStatus`][readdevicestatus]), or this error
occurred on a previous boot and a system update has not been applied since. A
corresponding notification [must][boot_flow_red_eio] be shown to obtain user
confirmation before proceeding with the dialogs for other colors and boot in
degraded mode, allowing the device to receive a future update that resolves the
issue.

#### GBL_EFI_AVB_PROPERTY

```c
typedef struct {
  CONST CHAR8 *BasePartitionName;
  CONST CHAR8 *Key;
  UINTN       ValueSize;
  CONST UINT8 *Value;
} GBL_EFI_AVB_PROPERTY;
```

##### BasePartitionName

A pointer to a null-terminated UTF-8 slotless partition name (e.g `vbmeta` for
`vbmeta_a`).

##### Key

Pointer to a null-terminated UTF-8 string representing the property key name.

##### ValueSize

Size of the provided property `Value` buffer, excluding a null terminator.

##### Value

Points to a buffer containing the property value of `ValueSize` bytes.
Guaranteed to be followed by a null terminator.

#### GBL_EFI_AVB_LOADED_PARTITION

```c
typedef struct {
  CONST CHAR8 *BaseName;
  UINTN       DataSize;
  CONST UINT8 *Data;
} GBL_EFI_AVB_LOADED_PARTITION;
```

##### BaseName

A pointer to a null-terminated UTF-8 slotless partition name (e.g `custom` for
`custom_a`).

##### DataSize

Size of the loaded partition `Data` buffer.

##### Data

Points to a buffer containing the loaded partition data of `DataSize` bytes.

#### GBL_EFI_AVB_VERIFICATION_RESULT

```c
typedef struct {
  GBL_EFI_AVB_BOOT_COLOR_FLAGS       ColorFlags;
  CONST CHAR8                        *Digest;
  UINTN                              NumPartitions;
  CONST GBL_EFI_AVB_LOADED_PARTITION *Partitions;
  UINTN                              NumProperties;
  CONST GBL_EFI_AVB_PROPERTY         *Properties;
  UINT32                             Reserved[8];
} GBL_EFI_AVB_VERIFICATION_RESULT;
```

##### ColorFlags

Represents the verification result using `GBL_EFI_AVB_BOOT_COLOR_FLAGS`. Only
one of the following may be set at a time to indicate the boot state for
firmware to request user confirmation, as required by the boot flow
[documentation][boot_flow]:

1. `GBL_EFI_AVB_BOOT_COLOR_RED`
2. `GBL_EFI_AVB_BOOT_COLOR_ORANGE`
3. `GBL_EFI_AVB_BOOT_COLOR_YELLOW`
4. `GBL_EFI_AVB_BOOT_COLOR_GREEN`

`GBL_EFI_AVB_BOOT_COLOR_RED_EIO` may be set alongside `ORANGE`, `YELLOW` and
`GREEN` colors to indicate that a dm-verity (hash tree) error occurred,
requiring an additional user confirmation [dialog][boot_flow_red_eio].

See the corresponding section above for more details.

##### Digest

Points to null-terminated UTF-8 hex string with the result digest calculated by
the `libavb`.

##### NumPartitions

The number of verified partitions referenced by the `Partitions` array. May be
`0` if verification fails (so `RED` state color).

##### Partitions

Pointer to an array of `NumPartitions` `GBL_EFI_AVB_LOADED_PARTITION` items,
each containing a verified partition content. May be `NULL` if verification
fails (so `RED` state color).

##### NumProperties

The number of properties contained in the `Properties` array. May be `0` if no
properties are present in the partition data or if verification fails (so `RED`
state color).

##### Properties

Pointer to an array of `NumProperties` `GBL_EFI_AVB_PROPERTY` items containing
all AVB properties extracted from `vbmeta` and chained partition footers. May be
`NULL` if no properties are provided or verification fails (so `RED` state
color).

##### Reserved

Reserved for potential future use cases.

### Parameters

#### Self

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### Result

Verification result state along with corresponding metadata to be handled by the
firmware. See related definitions from above for more details.

### Description

Regardless of the verification result, GBL invokes this method to allow the
firmware to handle it along with the provided metadata. It is intended to be
used for:

1. Update the root of trust along with the device state.
2. Handle anti-tampering mechanisms.
3. Handle data for all partitions loaded by GBL, including device-specific
   partitions requested through
   [`ReadPartitionsToVerify()`][readpartitionstoverify].
4. Display the appropriate UI and obtaining user confirmation for states
   that may affect the device's security guarantees.

Note: The data pointed to by `Result` (including the loaded partitions and
properties buffers) is valid only for the duration of this call and becomes
invalid afterward.

### Status Codes Returned

|||
| --- | --- |
| `EFI_SUCCESS` | Verification result is successfully handled. |
| `EFI_INVALID_PARAMETER` | Invalid data is provided by the `Result`. GBL rejects to boot. |
| `EFI_ACCESS_DENIED` | Failed to update root of trust or other secure world issues occurred. GBL rejects to boot. |

## Status codes returned to `libavb`

Some of the methods across this protocol are initiated by the `libavb`. The
following UEFI error codes are used to communicate results back to the library:

|                                |                                                                                                                                                          |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`                  | Requested operation was successful `libavb::AvbIOResult::AVB_IO_RESULT_OK`                                                                               |
| `EFI_OUT_OF_RESOURCES`         | Unable to allocate memory `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_OOM`                                                                                 |
| `EFI_DEVICE_ERROR`             | Underlying hardware (disk or other subsystem) encountered an I/O error `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_IO`                                     |
| `EFI_NOT_FOUND`                | Named persistent value or rollback index does not exist for the corresponding key `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_NO_SUCH_VALUE`               |
| `EFI_END_OF_FILE`              | Range of bytes requested to be read or written is outside the range of the partition `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_RANGE_OUTSIDE_PARTITION`  |
| `EFI_INVALID_PARAMETER`        | Named persistent value size is not supported or does not match the expected size `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_INVALID_VALUE_SIZE`           |
| `EFI_BUFFER_TOO_SMALL`         | Buffer is too small for the requested operation `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_INSUFFICIENT_SPACE`                                            |
| `EFI_UNSUPPORTED`              | Operation isn't implemented / supported                                                                                                                  |
| Others                         | Treated as `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_IO`                                                                                                 |

[readpartitionstoverify]: #gbl_efi_avb_protocolreadpartitionstoverify
[readdevicestatus]: #gbl_efi_avb_protocolreaddevicestatus
[handleverificationresult]: #gbl_efi_avb_protocolhandleverificationresult
[protocolwriterollbackindex]: #gbl_efi_avb_protocolwriterollbackindex
[avb]: https://source.android.com/docs/security/features/verifiedboot/avb
[unlocked]: https://android.googlesource.com/platform/external/avb/+/refs/heads/main/README.md#locked-and-unlocked-mode
[dmv_error]: https://android.googlesource.com/platform/external/avb/+/master/README.md#handling-dm_verity-errors
[rp]: https://android.googlesource.com/platform/external/avb/+/android16-release/README.md#rollback-protection
[update_ri]: https://android.googlesource.com/platform/external/avb/+/android16-release/README.md#updating-stored-rollback-indexes
[pd]: https://android.googlesource.com/platform/external/avb/+/android16-release/README.md#persistent-digests
[boot_flow]: https://source.android.com/docs/security/features/verifiedboot/boot-flow
[boot_flow_red]: https://source.android.com/docs/security/features/verifiedboot/boot-flow#no-valid-os-found
[boot_flow_orange]: https://source.android.com/docs/security/features/verifiedboot/boot-flow#unlocked-devices
[boot_flow_yellow]: https://source.android.com/docs/security/features/verifiedboot/boot-flow#locked-devices-with-custom-root-of-trust
[boot_flow_red_eio]: https://source.android.com/docs/security/features/verifiedboot/boot-flow#dm-verity-corruption
