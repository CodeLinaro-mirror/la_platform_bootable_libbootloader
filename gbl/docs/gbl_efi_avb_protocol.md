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
#define GBL_EFI_AVB_PROTOCOL_REVISION 0x00000001
```

Note: Revisions below 0x00010000 indicate that the protocol is not yet stable,
and backward compatibility is not guaranteed.

### Protocol Interface Structure

```c
typedef struct _GBL_EFI_AVB_PROTOCOL {
  UINT64 Revision;
  GBL_EFI_AVB_READ_PARTITIONS_TO_VERIFY ReadPartitionsToVerify;
  GBL_EFI_AVB_READ_IS_DM_VERITY_ERROR ReadIsDmVerityError;
  GBL_EFI_AVB_VALIDATE_VBMETA_PUBLIC_KEY ValidateVbmetaPublicKey;
  GBL_EFI_AVB_READ_IS_DEVICE_UNLOCKED ReadIsDeviceUnlocked;
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

#### ReadIsDmVerityError

Retrieves whether the device is rebooted due to dm-verity error.
[`ReadIsDmVerityError()`](#gbl_efi_avb_protocolreadisdmverityerror).

#### ValidateVbmetaPublicKey

Validates proper public key is used to sign HLOS artifacts.
[`ValidateVbmetaPublicKey()`](#gbl_efi_avb_protocolvalidatevbmetapublickey).

#### ReadIsDeviceUnlocked

Retrieves whether the device is unlocked.
[`ReadIsDeviceUnlocked()`](#gbl_efi_avb_protocolreadisdeviceunlocked).

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
[`HandleVerificationResult()`](#gbl_efi_avb_protocolhandleverificationresult).

## GBL_EFI_IMAGE_LOADING_PROTOCOL.ReadPartitionsToVerify()

### Summary

Retrieves the list of additional partitions to be verified, beyond the standard
set loaded and verified by GBL.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_READ_PARTITIONS_TO_VERIFY) (
  IN GBL_EFI_AVB_PROTOCOL *This,
  IN OUT UINTN *NumberOfPartitions,
  IN OUT GBL_EFI_AVB_PARTITION *Partitions,
);
```

### Parameters

#### This

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### NumberOfPartitions

Number of `Partitions` available to be filled by the FW. Must be updated to the
number of partitions returned. If there are no extra partitions to be verified,
`NumberOfPartitions` must be set to 0.

#### Partitions

Pointer to an array of [`GBL_EFI_AVB_PARTITION`](#gbl_efi_avb_partition) with
`NumberOfPartitions` elements, to be filled by the FW with additional partitions
that GBL will load and verify.

### Description

GBL loads and verifies a default set of partitions required to boot the HLOS.
For example, in case of Android, GBL loads and verifies the following standard
set of partitions: `boot`, `init_boot`, `vendor_boot`, `vendor_kernel_boot`,
`dtb`, `dtbo`, and `pvmfw`, which are used to boot the system.

This method allows the firmware specify extra non-standard partitions that GBL
will also load and verify to extend the integrity check.

For example, to provide N additional partitions, firmware must update the
`NumberOfPartitions` to N and fill first N elements of `Partitions` following
the [`GBL_EFI_AVB_PARTITION`](#gbl_efi_avb_partition) format. If no extra
partitions are required to be verified, `NumberOfPartitions` must be set to 0 or
`EFI_UNSUPPORTED` is returned.

If a requested partition does not have a corresponding hash descriptor in
`vbmeta` or chained partition then it cannot be verified. GBL will treat it the
following way:

1. For a locked device: `RED` boot status color, so fail to boot.
2. For an unlocked device: `ORANGE` boot status color, still can boot.

### Status Codes Returned

|||
| --- | --- |
| `EFI_SUCCESS` | Successfully provided additional partitions to verify |
| `EFI_UNSUPPORTED` | No extra partitions need to be verified |
| `EFI_BUFFER_TOO_SMALL` | Provided list of `Partitions` is too small; `NumberOfPartitions` has been updated with the required amount. GBL will call this method again with extended `Partitions`. |
| `EFI_BAD_BUFFER_SIZE` | One of provided `Partition.NameLen` values is not sufficient to hold the partition name. GBL will fail to boot. |

### Related Definitions

#### GBL_EFI_AVB_PARTITION

```c
typedef
struct GblEfiAvbPartition {
  UINTN BaseNameLen;
  UINT8* BaseName;
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
terminator is not required be included.

## GBL_EFI_AVB_PROTOCOL.ReadIsDmVerityError()

### Summary

Allows the firmware to provide the dm-verity error state to GBL in a
firmware-specific way.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_READ_IS_DM_VERITY_ERROR) (
  IN GBL_EFI_AVB_PROTOCOL *This,
  OUT BOOLEAN *IsDmVerityError);
```

### Parameters

#### This

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### IsDmVerityError

An output parameter that communicates the dm-verity error state to GBL.

### Description

If `IsDmVerityError` is set `True` by the FW, GBL will pass
[`AVB_SLOT_VERIFY_FLAGS_RESTART_CAUSED_BY_HASHTREE_CORRUPTION`][dmv_error], so
`RED_IO` status will be reported unless new OS images are detected by the
`libavb`.

## GBL_EFI_AVB_PROTOCOL.ValidateVbmetaPublicKey()

### Summary

Allows the firmware to verify whether the public key used to sign the `vbmeta`
partition is trusted by checking it against the hardware-trusted key shipped
with the device.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_VALIDATE_VBMETA_PUBLIC_KEY) (
  IN GBL_EFI_AVB_PROTOCOL *This,
  IN CONST UINT8 *PublicKeyData,
  IN UINTN PublicKeyLength,
  IN CONST UINT8 *PublicKeyMetadata,
  IN UINTN PublicKeyMetadataLength,
  /* GBL_EFI_AVB_KEY_VALIDATION_STATUS */ OUT UINT32 *ValidationStatus);
```

### Parameters

#### This

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### PublicKeyData

A pointer to the public key extracted from `vbmeta`. Guaranteed to contain valid
data of length `PublicKeyLength`.

#### PublicKeyLength

Specifies the length of the public key provided by `PublicKeyData`.

#### PublicKeyMetadata

A pointer to public key metadata provided using the `avbtool`'s
`--public_key_metadata` flag. May be `NULL` if no public key metadata is
provided.

#### PublicKeyMetadataLength

Specifies the length of the public key metadata provided by `PublicKeyMetadata`.
Guaranteed to be 0 in case of `NULL` `PublicKeyMetadata`.

#### ValidationStatus

An output parameter that communicates the verification status to GBL. `VALID`
and `VALID_CUSTOM_KEY` are interpreted as successful validation statuses.

### Related Definitions

```c
// Vbmeta key validation status.
//
// https://source.android.com/docs/security/features/verifiedboot/boot-flow#locked-devices-with-custom-root-of-trust
typedef enum {
  VALID,
  VALID_CUSTOM_KEY,
  INVALID,
} GBL_EFI_AVB_KEY_VALIDATION_STATUS;
```

### Description

`ValidateVbmetaPublicKey` must set `ValidationStatus` and return `EFI_SUCCESS`.
Any return value other than `EFI_SUCCESS` is treated as a fatal verification
error, resulting in a `red` state being reported and GBL failing to boot, even
if the device is unlocked.

**`ValidationStatus` and GBL boot flow**:

* `VALID`: The public key is valid and trusted, so the device can continue the
  boot process for both locked and unlocked states.

* `VALID_CUSTOM_KEY`: The public key is valid but not fully trusted. GBL
  continues booting a locked device with a `yellow` state and an unlocked device
  with an `orange` state.

* `INVALID`: The public key is not valid. The device cannot continue the boot
  process for locked devices; GBL reports a `red` status and resets. Unlocked
  devices can still boot with an `orange` state.

GBL calls this function once per AVB verification session.

## GBL_EFI_AVB_PROTOCOL.ReadIsDeviceUnlocked()

### Summary

Allows the firmware to provide the device's locking state to GBL in a
firmware-specific way.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_READ_IS_DEVICE_UNLOCKED) (
  IN GBL_EFI_AVB_PROTOCOL *This,
  OUT BOOLEAN *IsUnlocked);
```

### Parameters

#### This

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### IsUnlocked

An output parameter that communicates the device locking state to GBL.

### Description

An unlocked device state allows GBL not to force AVB and to boot the device with
an `orange` boot state. GBL rejects continuing the boot process if this method
returns any error. GBL may call this method multiple times per boot session.

## GBL_EFI_AVB_PROTOCOL.ReadRollbackIndex()

### Summary

Allows the firmware to provide rollback index for the provided index location to
GBL in a vendor-specific way.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_READ_ROLLBACK_INDEX) (
  IN GBL_EFI_AVB_PROTOCOL *This,
  IN USIZE IndexLocation,
  OUT UINT64 *RollbackIndex);
```

### Parameters

#### This

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

## GBL_EFI_AVB_PROTOCOL.WriteRollbackIndex()

### Summary

Allows the firmware to update rollback index for the provided index location in
a vendor-specific way.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_WRITE_ROLLBACK_INDEX) (
  IN GBL_EFI_AVB_PROTOCOL *This,
  IN USIZE IndexLocation,
  IN UINT64 RollbackIndex);
```

### Parameters

#### This

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### IndexLocation

The location of the rollback index to be set by this method.

#### RollbackIndex

A rollback index value to be set for the provided `IndexLocation`.

### Description

For a locked device, if a known-good slot is successfully verified, GBL updates
rollback indexes to the value provided in the vbmeta header in accordance with
`libavb` [requrements][update_ri]. This prevents a locked device from booting a
previous version of HLOS on the next boot, ensuring [rollback protection][rp] in
case of an OTA.

GBL only updates rollback indexes for `IndexLocation` equals `0` as a global
HLOS index or locations specified in the corresponding chained partition
descriptors. Returning any error in such cases causes GBL boot failure for
locked devices.

## GBL_EFI_AVB_PROTOCOL.ReadPersistentValue()

### Summary

Allows the firmware to read a persistent value associated with the given name in
a vendor-specific manner.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_READ_PERSISTENT_VALUE) (
  IN GBL_EFI_AVB_PROTOCOL *This,
  IN CONST CHAR8 *Name,
  OUT UINT8 *Value,
  IN OUT USIZE *ValueSize);
```

### Parameters

#### This

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### Name

Points to null-terminated UTF-8 name for the requested persistent value.

#### Value

Points to the buffer of `ValueSize` bytes to be filled by FW with a requested
value. May be `NULL` if GBL only wants to check value's availability.

#### ValueSize

On input, points to the size of the provided `Value` buffer, or `0` if GBL only
wants to check the value's availability. On output, the firmware should update
it to reflect the actual size of the value.

### Description

GBL requests `avb.persistent_digest.<partition_name>` persistent values to
support the [persistent digest][pd] `libavb` feature. Additionally, GBL may
request the `avb.managed_verity_mode` persistent value to detect HLOS updates to
handle [dm-verity][dmv_error] errors and EIO mode.

### Status Codes Returned

|||
| --- | --- |
| `EFI_SUCCESS` | The requested persistent value is presented and succesfully provided in case `Value` buffer isn't NULL. |
| `EFI_STATUS_NOT_FOUND` | The requested persistent value is not yet populated or supported. GBL will try to initialize it using `WritePersistentValue`. |
| `EFI_STATUS_BUFFER_TOO_SMALL` | The provided `Value` buffer is too small. GBL rejects to boot. |
| `EFI_STATUS_INVALID_PARAMETER` | Unexpected arguments combination. GBL rejects to boot. |

## GBL_EFI_AVB_PROTOCOL.WritePersistentValue()

### Summary

Allows the firmware to write a persistent value for the provided name in a
vendor-specific way.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_WRITE_PERSISTENT_VALUE) (
  IN GBL_EFI_AVB_PROTOCOL *This,
  IN CONST CHAR8 *Name,
  IN CONST UINT8 *Value,
  IN USIZE ValueSize);
```

### Parameters

#### This

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### Name

Points to null-terminated UTF-8 name for the persistent value to update.

#### Value

Points to a buffer of `ValueSize` bytes containing the value to set.

#### ValueSize

Points to the size of the `Value` to be set. May be `0`, in which case the
corresponding persistent value must be treated as not present after such
operation.

### Description

GBL initializes `avb.persistent_digest.<partition_name>` persistent values to
support the [persistent digest][pd] `libavb` feature. Additionally, if a
[dm-verity][dmv_error] error occurs, GBL updates the `avb.managed_verity_mode`
persistent value with the current vbmeta digest. This allows detection of HLOS
updates in order to disable EIO mode.

### Status Codes Returned

|||
| --- | --- |
| `EFI_SUCCESS` | The value for `Name` is succesfully updated. |
| `EFI_STATUS_NOT_FOUND` | The value for `Name` isn't supported. |
| `EFI_STATUS_INVALID_PARAMETER` | The `ValueSize` is too big or any other unexpected arguments combination. GBL rejects to boot. |

## GBL_EFI_AVB_PROTOCOL.HandleVerificationResult()

### Summary

Allows the firmware to handle the verification result in a vendor-specific
manner.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_HANDLE_VERIFICATION_RESULT) (
  IN GBL_EFI_AVB_PROTOCOL *This,
  IN CONST GBL_EFI_AVB_VERIFICATION_RESULT *Result);
```

### Related Definitions

#### GBL_EFI_AVB_BOOT_STATE_COLOR

```c
typedef enum {
  GBL_EFI_AVB_BOOT_STATE_GREEN,
  GBL_EFI_AVB_BOOT_STATE_YELLOW,
  GBL_EFI_AVB_BOOT_STATE_ORANGE,
  GBL_EFI_AVB_BOOT_STATE_RED_EIO,
  GBL_EFI_AVB_BOOT_STATE_RED,
} GBL_EFI_AVB_BOOT_STATE_COLOR;
```

##### STATE_GREEN

Device is locked and verification passed. Boot can proceed.

##### STATE_YELLOW

Device is locked and verification passed using a user-provided custom key. A
corresponding notification must be shown to obtain user confirmation before
proceeding with the boot.

##### STATE_ORANGE

Used regardless of the verification result to indicate that the device is
unlocked. A corresponding notification must be shown to obtain user confirmation
before proceeding with the boot. HLOS functionality may be limited.

##### STATE_RED_EIO

A dm-verity [error][dmv_error] has been detected. A corresponding notification
must be shown to obtain user confirmation before proceeding with the boot in
degraded mode, allowing the device to receive a future update that resolves the
issue.

##### STATE_RED

Verification failed (including fatal errors on an unlocked device). Boot cannot
proceed.

#### GBL_EFI_AVB_VERIFICATION_RESULT

```c
typedef struct {
  // GBL_EFI_AVB_BOOT_STATE_COLOR
  UINT32       Color;
  CONST CHAR8  *Digest;

  CONST CHAR8  *BootVersion;
  CONST CHAR8  *BootSecurityPatch;
  CONST CHAR8  *SystemVersion;
  CONST CHAR8  *SystemSecurityPatch;
  CONST CHAR8  *VendorVersion;
  CONST CHAR8  *VendorSecurityPatch;
} GBL_EFI_AVB_VERIFICATION_RESULT;
```

##### Color

The verification result `GBL_EFI_AVB_BOOT_STATE_COLOR`. See corresponding
section from above for more details.

##### Digest

Points to UTF-8 string with the result digest calculated by the `libavb`.

TODO(b/337846185): Add Digest len (along with other breaking changes) to make it
easier for vendors to diff between 64 and 32 bytes digest types.

##### BootVersion

Points to null-terminated UTF-8 string with `com.android.build.boot.os_version`
property value extracted from `boot` footer or `vbmeta` header. May be NULL in
case such property isn't presented or failed verification (so `RED` state
color).

##### BootSecurityPatch

Points to null-terminated UTF-8 string with
`com.android.build.boot.security_patch` property value extracted from `boot`
footer or `vbmeta` header. May be NULL in case such property isn't presented or
failed verification (so `RED` state color).

##### SystemVersion

Points to null-terminated UTF-8 string with
`com.android.build.system.os_version` property value extracted from
`vbmeta_system` or `vbmeta` headers. May be NULL in case such property isn't
presented or failed verification (so `RED` state color).

##### SystemSecurityPatch

Points to null-terminated UTF-8 string with
`com.android.build.system.security_patch` property value extracted from
`vbmeta_system` or `vbmeta` headers. May be NULL in case such property isn't
presented or failed verification (so `RED` state color).

##### VendorVersion

Points to null-terminated UTF-8 string with
`com.android.build.vendor.os_version` property value extracted from
`vbmeta_vendor` or `vbmeta` headers. May be NULL in case such property isn't
presented or failed verification (so `RED` state color).

##### VendorSecurityPatch

Points to null-terminated UTF-8 string with
`com.android.build.vendor.security_patch` property value extracted from
`vbmeta_vendor` or `vbmeta` headers. May be NULL in case such property isn't
presented or failed verification (so `RED` state color).

### Parameters

#### This

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### Result

Verification result state along with corresponding metadata to be handled by the
firmware. See related definitions from above for more details.

### Description

Regardless of the verification result, GBL invokes this method to allow the
firmware to handle it along with the provided metadata. It is intended to be
used for:

1. Updating the root of trust along with the device state.
2. Handling anti-tampering mechanisms.
3. Displaying the appropriate UI and obtaining user confirmation for states
   that may affect the device's security guarantees.

Note: The data pointed to by `Result` is only valid during this call and becomes
unavailable afterward.

### Status Codes Returned

|||
| --- | --- |
| `EFI_SUCCESS` | Verification result is successfully handled. |
| `EFI_STATUS_INVALID_PARAMETER` | Invalid data is provided by the `Result`. GBL rejects to boot. |
| `EFI_ACCESS_DENIED` | Failed to update root of trust or other secure world issues occurred. GBL rejects to boot. |

## Status Codes Returned

The following UEFI error types are used to communicate result to GBL and
`libavb` in particular:

|                                |                                                                                                                                                         |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`                  | Requested operation was successful `libavb::AvbIOResult::AVB_IO_RESULT_OK`                                                                              |
| `EFI_STATUS_OUT_OF_RESOURCES`  | Unable to allocate memory `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_OOM`                                                                                |
| `EFI_STATUS_DEVICE_ERROR`      | Underlying hardware (disk or other subsystem) encountered an I/O error `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_IO`                                    |
| `EFI_STATUS_NOT_FOUND`         | Named persistent value or rollback index does not exist for the corresponding key `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_NO_SUCH_VALUE`              |
| `EFI_STATUS_END_OF_FILE`       | Range of bytes requested to be read or written is outside the range of the partition `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_RANGE_OUTSIDE_PARTITION` |
| `EFI_STATUS_INVALID_PARAMETER` | Named persistent value size is not supported or does not match the expected size `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_INVALID_VALUE_SIZE`          |
| `EFI_STATUS_BUFFER_TOO_SMALL`  | Buffer is too small for the requested operation `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_INSUFFICIENT_SPACE`                                           |
| `EFI_STATUS_UNSUPPORTED`       | Operation isn't implemented / supported                                                                                                                 |

TODO(b/337846185): Split status codes returned on per-method basis.

[readpartitionstoverify]: #gbl_efi_image_loading_protocolreadpartitionstoverify
[protocolwriterollbackindex]: #gbl_efi_avb_protocolwriterollbackindex
[avb]: https://source.android.com/docs/security/features/verifiedboot/avb
[dmv_error]: https://android.googlesource.com/platform/external/avb/+/master/README.md#handling-dm_verity-errors
[rp]: https://android.googlesource.com/platform/external/avb/+/android16-release/README.md#rollback-protection
[update_ri]: https://android.googlesource.com/platform/external/avb/+/android16-release/README.md#updating-stored-rollback-indexes
[pd]: https://android.googlesource.com/platform/external/avb/+/android16-release/README.md#persistent-digests
