# GBL AVB EFI Protocol

## GBL_EFI_AVB_PROTOCOL

### Summary

This protocol allows to delegate device-specific Android verified booot (AVB)
logic to the firmware.

`GBL_EFI_AVB_PROTOCOL` protocol isn't required for dev GBL flavour with
intention to support basic Android boot functionality on dev boards. On the
production devices this protocol must be provided by the FW to ensure HLOS
integrity.

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

Note: revision 0 means the protocol is not yet stable and may change in
backwards-incompatible ways.

```c
#define GBL_EFI_AVB_PROTOCOL_REVISION 0x00010000
```

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

Gets the list of additional partitions to be verified, beyond the standard set
loaded and verified by GBL.
[`GetVerifyPartitions()`](#gbl_efi_image_loading_protocolreadpartitionstoverify).

#### ReadIsDmVerityError

Gets whether the device is rebooted due to dm-verity error.
[`ReadIsDmVerityError()`](#gbl_efi_avb_protocolreadisdmverityerror).

#### ValidateVbmetaPublicKey

Validate proper public key is used to sign HLOS artifacts.
[`ValidateVbmetaPublicKey()`](#gbl_efi_avb_protocolvalidatevbmetapublickey).

#### ReadIsDeviceUnlocked

Gets whether the device is unlocked.
[`ReadIsDeviceUnlocked()`](#gbl_efi_avb_protocolreadisdeviceunlocked).

#### ReadRollbackIndex

Gets the rollback index corresponding to the location given by `index_location`.
[`ReadRollbackIndex()`](#readrollbackindex).

#### WriteRollbackIndex

Sets the rollback index corresponding to the location given by `index_location` to `rollback_index`.
[`WriteRollbackIndex()`](#writerollbackindex).

#### ReadPersistentValue

Gets the persistent value for the corresponding `name`.
[`ReadPersistentValue()`](#readpersistentvalue).

#### WritePersistentValue

Sets or erases the persistent value for the corresponding `name`.
[`WritePersistentValue()`](#writepersistentvalue).

#### HandleVerificationResult

Handle AVB verification result (i.e update ROT, set device state, display UI
warnings/errors, handle anti-tampering, etc).
[`HandleVerificationResult()`](#handleverificationresult).

## GBL_EFI_IMAGE_LOADING_PROTOCOL.ReadPartitionsToVerify()

### Summary

Gets the list of additional partitions to be verified, beyond the standard set
loaded and verified by GBL.

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
`NumberOfPartitions` should be set to 0.

#### Partitions

Pointer to an array of [`GBL_EFI_AVB_PARTITION`](#gbl_efi_avb_partition) with
`NumberOfPartitions` elements, to be filled by the FW with additional partitions
that GBL will load and verify.

### Description

GBL loads and verifies a default set of partitions needed to boot HLOS. For
example, in case of Android, GBL loads and verifies the following standard
partitions: `boot`, `init_boot`, `vendor_boot`, `vendor_kernel_boot`, `dtb`,
`dtbo`, `pvmfw` used to boot the system.

This method allows the firmware specify extra non-standard partitions that GBL
will also load and verify to extend the integrity check.

For example, to provide N additional partitions, the first N elements of
`Partitions` must be filled with names, following the
[`GBL_EFI_AVB_PARTITION`](#gbl_efi_avb_partition) format. If no extra partitions
are required, `NumberOfPartitions` should be set to 0 or `EFI_UNSUPPORTED`
should be returned.

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
  UINTN NameLen;
  UINT8* Name;
} GBL_EFI_AVB_PARTITION;
```

##### NameLen

On input, represents the size of available space pointed by `Name` to be filled
with UTF-8 partition name. On output, must be updated to the partition name's
length copied into `Name` buffer.

##### Name

A pointer to available buffer of `NameLen` size for UTF-8 partition name to be
copied by FW implementation. A copied partition name must not be null-terminated
and must not include the slot suffix.

## GBL_EFI_AVB_PROTOCOL.ReadIsDmVerityError()

### Summary

Allows the firmware to provide the dm-verity error occurred to the GBL in a
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

An output parameter that communicates the dm-verity error state to the GBL.

### Description

If `IsDmVerityError` is set `True` by the FW, GBL will pass [`AVB_SLOT_VERIFY_FLAGS_RESTART_CAUSED_BY_HASHTREE_CORRUPTION`][dm_verity_error],
so `RED_IO` status will be reported unless new OS images are detected by the
`libavb`.

## GBL_EFI_AVB_PROTOCOL.ValidateVbmetaPublicKey()

### Summary

Allows the firmware to check whether the public key used to sign the `vbmeta`
partition is trusted by verifying it against the hardware-trusted key shipped
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

A pointer to public key metadata generated using the `avbtool` `--public_key_metadata`
flag. May be `NULL` if no public key metadata is provided.

#### PublicKeyMetadataLength

Specifies the length of the public key metadata provided by `PublicKeyMetadata`.
Guaranteed to be 0 in case of `NULL` `PublicKeyMetadata`.

#### ValidationStatus

An output parameter that communicates the verification status to the GBL. `VALID`
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
Any non `EFI_SUCCESS` return value from this method is treated as a fatal verification
error, so `red` state is reported and GBL fails to boot even if device is unlocked.

**`ValidationStatus` and GBL boot flow**:

* `VALID`: The public key is valid and trusted, so the device can continue the boot
  process for both locked and unlocked states.

* `VALID_CUSTOM_KEY`: The public key is valid but not fully trusted. GBL continues
  booting a locked device with a `yellow` state and an unlocked device with an `orange` state.

* `INVALID`: The public key is not valid. The device cannot continue the boot process
  for locked devices; GBL reports a `red` status and resets. Unlocked devices can still
  boot with an `orange` state.

GBL calls this function once per AVB verification session.

## GBL_EFI_AVB_PROTOCOL.ReadIsDeviceUnlocked()

### Summary

Allows the firmware to provide the device's locking state to the GBL in a
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

An output parameter that communicates the device locking state to the GBL.

### Description

An unlocked device state allows GBL not to force AVB and to boot the device with
an `orange` boot state. GBL rejects continuing the boot process if this method
returns any error. GBL may call this method multiple times per boot session.

## Status Codes Returned

The following EFI error types are used to communicate result to GBL and libavb in particular:

|                                |                                                                                                                                                         |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`                  | Requested operation was successful `libavb::AvbIOResult::AVB_IO_RESULT_OK`                                                                              |
| `EFI_STATUS_OUT_OF_RESOURCES`  | Unable to allocate memory `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_OOM`                                                                                |
| `EFI_STATUS_DEVICE_ERROR`      | Underlying hardware (disk or other subsystem) encountered an I/O error `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_IO`                                    |
| `EFI_STATUS_NOT_FOUND`         | Named persistent value does not exist `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_NO_SUCH_VALUE`                                                          |
| `EFI_STATUS_END_OF_FILE`       | Range of bytes requested to be read or written is outside the range of the partition `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_RANGE_OUTSIDE_PARTITION` |
| `EFI_STATUS_INVALID_PARAMETER` | Named persistent value size is not supported or does not match the expected size `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_INVALID_VALUE_SIZE`          |
| `EFI_STATUS_BUFFER_TOO_SMALL`  | Buffer is too small for the requested operation `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_INSUFFICIENT_SPACE`                                           |
| `EFI_STATUS_UNSUPPORTED`       | Operation isn't implemented / supported                                                                                                                 |

TODO(b/337846185): Provide docs for all methods.

[dm_verity_error]: https://android.googlesource.com/platform/external/avb/+/master/README.md#handling-dm_verity-errors
