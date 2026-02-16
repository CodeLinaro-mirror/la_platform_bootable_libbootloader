# GBL EFI Android Verified Boot Protocol

|             |            |
| :---------- | :--------- |
| **Status**  | Pre-frozen |
| **Created** | 2024-11-15 |

## GBL_EFI_AVB_PROTOCOL

### Summary

Android Verified Boot ([AVB][avb]) is a process of assuring the end user of the
integrity of the software running on a device. This protocol allows
vendor-specific [AVB][avb] logic to be implemented by the firmware, enabling
device-specific security mechanisms to ensure the integrity of the HLOS.

The `GBL_EFI_AVB_PROTOCOL` is not required for the development GBL flavor, which
is intended to support basic Android boot functionality on unlocked development
boards. However, this protocol must be implemented on production devices.

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
#define GBL_EFI_AVB_PROTOCOL_REVISION GBL_PROTOCOL_REVISION(0, 256)
```

See
[GBL Custom Protocol Revisions](efi_protocols.md#gbl-custom-protocol-revisions)
for details about protocol revisions.

### Protocol Interface Structure

```c
typedef struct _GBL_EFI_AVB_PROTOCOL {
  UINT64 Revision;
  GBL_EFI_AVB_READ_PARTITION_ATTRIBUTES ReadPartitionAttributes;
  GBL_EFI_AVB_READ_DEVICE_STATUS ReadDeviceStatus;
  GBL_EFI_AVB_VALIDATE_VBMETA_PUBLIC_KEY ValidateVbmetaPublicKey;
  GBL_EFI_AVB_READ_ROLLBACK_INDEX ReadRollbackIndex;
  GBL_EFI_AVB_WRITE_ROLLBACK_INDEX WriteRollbackIndex;
  GBL_EFI_AVB_READ_PERSISTENT_VALUE ReadPersistentValue;
  GBL_EFI_AVB_WRITE_PERSISTENT_VALUE WritePersistentValue;
  GBL_EFI_AVB_HANDLE_VERIFICATION_RESULT HandleVerificationResult;
  GBL_EFI_AVB_WRITE_LOCK_STATE WriteLockState;
  GBL_EFI_AVB_FACTORY_DATA_RESET FactoryDataReset;
} GBL_EFI_AVB_PROTOCOL;
```

### Parameters

#### Revision

The revision to which the `GBL_EFI_AVB_PROTOCOL` adheres. All future revisions
must be backwards compatible. If a future version is not backwards compatible, a
different GUID must be used.

#### ReadPartitionAttributes

Retrieves attributes for partitions that require custom handling. See
[`ReadPartitionAttributes()`][readpartitionattributes] for more information.

#### ReadDeviceStatus

Retrieves the current device status, including its lock state and dm-verity
error indication. See [`ReadDeviceStatus()`][readdevicestatus] for more
information.

#### ValidateVbmetaPublicKey

Validates proper public key is used to sign HLOS artifacts. See
[`ValidateVbmetaPublicKey()`][validatevbmetapublickey] for more information.

#### ReadRollbackIndex

Retrieves the rollback index corresponding to the provided index location. See
[`ReadRollbackIndex()`][readrollbackindex] for more information.

#### WriteRollbackIndex

Writes the rollback index corresponding to the provided index location. See
[`WriteRollbackIndex()`][writerollbackindex] for more information.

#### ReadPersistentValue

Retrieves the persistent value for the provided name. See
[`ReadPersistentValue()`][readpersistentvalue] for more information.

#### WritePersistentValue

Writes or clears the persistent value for the provided name. See
[`WritePersistentValue()`][writepersistentvalue] for more information.

#### HandleVerificationResult

Handles the AVB verification result (e.g., updating the Root of Trust, setting
device state, displaying UI warnings/errors, handling anti-tampering, etc.). See
[`HandleVerificationResult()`][handleverificationresult] for more information.

#### WriteLockState

Locks or unlocks the device lock or device critical lock. See
[`WriteLockState()`][writelockstate] for more information.

#### FactoryDataReset

Performs a factory data reset (FDR), securely erasing all user data. See
[`FactoryDataReset()`][factorydatareset] for more information.

## GBL_EFI_AVB_PROTOCOL.ReadPartitionAttributes()

### Summary

Provides attributes for any partitions that require special handling. This does
not need to be an exhaustive list of partitions, only partitions that have
special behavior indicated by the provided flags need to be provided here.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_READ_PARTITION_ATTRIBUTES) (
  IN GBL_EFI_AVB_PROTOCOL *Self,
  IN OUT UINTN *NumPartitions,
  IN OUT GBL_EFI_AVB_PARTITION_ATTRIBUTES *Partitions);
```

### Related Definitions

#### GBL_EFI_AVB_PARTITION_ATTRIBUTES

```
typedef struct {
  UINTN BaseNameLen,
  CHAR8 *BaseName,
  GBL_EFI_AVB_PARTITION_FLAGS Flags
} GBL_EFI_AVB_PARTITION_ATTRIBUTES;
```

##### BaseNameLen

On input, the length of the `BaseName` buffer. On output, the length of the data
copied into `BaseName` without termination.

##### BaseName

Points to a buffer of size `BaseNameLen`. On output, the buffer should be filled
with the base (slotless) partition name as UTF-8, e.g. `boot` rather than
`boot_a`.

Termination is not required, and no embedded terminators are allowed within the
output `BaseNameLen`.

##### Flags

The set of flags indicating any special handling required for this partition.
Must be some combination of defined `GBL_EFI_AVB_PARTITION_FLAG_*` constants,
with all unused bits set to 0.

#### GBL_EFI_AVB_PARTITION_FLAGS

```c
typedef UINT64 GBL_EFI_AVB_PARTITION_FLAGS;
STATIC CONST GBL_EFI_AVB_PARTITION_FLAGS GBL_EFI_AVB_PARTITION_FLAG_VERIFY = 0x1 << 0;
STATIC CONST GBL_EFI_AVB_PARTITION_FLAGS GBL_EFI_AVB_PARTITION_FLAG_VERIFY_IF_EXISTS = 0x1 << 1;
STATIC CONST GBL_EFI_AVB_PARTITION_FLAGS GBL_EFI_AVB_PARTITION_FLAG_FLASH_CRITICAL = 0x1 << 2;
STATIC CONST GBL_EFI_AVB_PARTITION_FLAGS GBL_EFI_AVB_PARTITION_FLAG_FDR = 0x1 << 3;
```

##### GBL_EFI_AVB_PARTITION_FLAG_VERIFY

This partition should be loaded and verified by libavb.

If a partition with this flag doesn't exist or lacks a corresponding hash
descriptor in `vbmeta` or a chained partition, it cannot be verified. GBL will
handle this case as follows:

1. For a locked device: `RED` boot status color, so fail to boot.
2. For an unlocked device: `ORANGE` boot status color, still can boot.

In addition to enforcing verification, this flag also makes the partition
available via [`HandleVerificationResult()`][handleverificationresult] once
verification is complete for backend-specific handling.

A partition cannot set both this and
`GBL_EFI_AVB_PARTITION_FLAG_VERIFY_IF_EXISTS`.

###### Defaults

GBL maintains a set of partitions that will always be verified. If these
partitions are provided to `ReadPartitionAttributes()` then they should **not**
specify `GBL_EFI_AVB_PARTITION_FLAG_VERIFY` or
`GBL_EFI_AVB_PARTITION_FLAG_VERIFY_IF_EXISTS`, or these partitions may be
verified twice which will slow boot and possibly allocate extra memory.

For Android these partitions are:

<!-- LINT.IfChange(always_verify_partitions) -->

- `boot`
- `dtb`
- `dtbo`
- `init_boot`
- `pvmfw`
- `vendor_boot`
- `vendor_kernel_boot`

<!-- LINT.ThenChange(/gbl/libgbl/src/android_boot/mod.rs:always_verify_partitions) -->

##### GBL_EFI_AVB_PARTITION_FLAG_VERIFY_IF_EXISTS

Similar to `GBL_EFI_AVB_PARTITION_FLAG_VERIFY`, but if this partition doesn't
exist or does not have a vbmeta hash descriptor it will be ignored rather than
causing a boot failure.

A partition cannot set both this and `GBL_EFI_AVB_PARTITION_FLAG_VERIFY`.

##### GBL_EFI_AVB_PARTITION_FLAG_FLASH_CRITICAL

This partition should be protected by the critical flashing lock. See
[`WriteLockState()`][writelockstate] for details.

It is up to the implementation to specify every desired critical partition; GBL
will not automatically apply the critical lock to any partitions.

##### GBL_EFI_AVB_PARTITION_FLAG_FDR

This partition is tied to Factory Data Reset (FDR). This has two effects:

1. Any fastboot write or erase of this partition will automatically be followed
   by a call to [`FactoryDataReset()`][factorydatareset].
2. GBL will use Block I/O protocols to erase all of these partitions (and issue
   a `FactoryDataReset()`) prior to changing device lock state via
   [`WriteLockState()`][writelockstate].

This functionality is provided mostly as a developer tool and must not be
security load-bearing, i.e. FDR must not assume or rely on any particular state
of non-secure storage. A common use case is to trigger FDR during `fastboot -w`
so that developers can easily reset their device state.

###### Defaults

By default, GBL will tie `userdata` and `metadata` partitions to FDR if they
exist. To opt out of this default behavior, the implementation can provide these
partitions in `ReadPartitionAttributes()` with this flag cleared.

It is recommended that at least one of `userdata`, `metadata`, or `cache`
partitions have `GBL_EFI_AVB_PARTITION_FLAG_FDR`. As of this writing,
`fastboot -w` attempts to reset these partitions, which means if none of these
partitions have the flag then `fastboot -w` will not trigger FDR.

### Parameters

#### Self

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

#### NumPartitions

On input, the number of `Partitions` available to be filled by the FW.

On output, with a return code of:

- `EFI_SUCCESS`: the number of `Partitions` filled by the implementation, less
  than or equal to the input `NumPartitions`
- `EFI_BUFFER_TOO_SMALL`: the number of `Partitions` that would be required
- other: `NumPartitions` will be ignored

#### Partitions

Pointer to an array of
[`GBL_EFI_AVB_PARTITION_ATTRIBUTES`](#gbl_efi_avb_partition_attributes) with
`NumPartitions` elements, to be filled by the implementation.

### Description

Provides attributes to indicate custom handling of the given partitions.

GBL provides some standard behaviors, but devices may want to customize which
behaviors those partitions apply to. This function allows specifying which
partitions get which behaviors.

The input is an array of empty partition attributes, and the output should be
the filled array. For example, to provide N additional partitions, firmware must
update the `NumPartitions` to N and fill the first N elements of `Partitions`
following the
[`GBL_EFI_AVB_PARTITION_ATTRIBUTES`](#gbl_efi_avb_partition_attributes) format.

If no partition attributes are needed, `NumPartitions` can be set to 0 or
`EFI_UNSUPPORTED` can be returned - both have the same effect.

### Status Codes Returned

| Return Code            | Semantics                                                                                               |
| :--------------------- | :------------------------------------------------------------------------------------------------------ |
| `EFI_SUCCESS`          | `NumPartitions` and the corresponding number of `Partitions` structs have been filled                   |
| `EFI_BUFFER_TOO_SMALL` | `NumPartitions` was not large enough for all the partitions and has been updated with the required size |
| `EFI_BAD_BUFFER_SIZE`  | One of the provided `Partitions.BaseNameLen` values was too small                                       |
| `EFI_UNSUPPORTED`      | Use the default partition attributes                                                                    |

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
STATIC CONST GBL_EFI_AVB_DEVICE_STATUS GBL_EFI_AVB_DEVICE_STATUS_UNLOCKED_CRITICAL = 0x1 << 2;
STATIC CONST GBL_EFI_AVB_DEVICE_STATUS GBL_EFI_AVB_DEVICE_STATUS_UNLOCKABLE = 0x1 << 3;
```

##### GBL_EFI_AVB_DEVICE_STATUS_UNLOCKED

Flag indicating that the device is unlocked.

##### GBL_EFI_AVB_DEVICE_STATUS_DM_VERITY_FAILED

Flag indicating that the device rebooted due to a dm-verity error.

##### GBL_EFI_AVB_DEVICE_STATUS_UNLOCKED_CRITICAL

Flag indicating that the device is unlocked for critical operations. These
operations include flashing raw storage devices and modifying partition tables.

##### GBL_EFI_AVB_DEVICE_STATUS_UNLOCKABLE

Flag indicating that the device bootloader can be unlocked. Corresponds to the
["OEM unlocking"][oem_unlocking] option in the booted OS.

The `UNLOCKABLE` status applies to both the
[`DEVICE`](#gbl_efi_avb_device_status_unlocked) lock and the
[`CRITICAL`](#gbl_efi_avb_device_status_unlocked_critical) lock.

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
   boot state, skipping certain verification enforcements and allowing boot to
   proceed with reduced security guarantees. See
   [unlocked_devices][boot_flow_orange].
2. `GBL_EFI_AVB_DEVICE_STATUS_DM_VERITY_FAILED` - Indicates the device rebooted
   due to a dm-verity hashtree corruption [error][dmv_error]. In this case, GBL
   passes `AVB_SLOT_VERIFY_FLAGS_RESTART_CAUSED_BY_HASHTREE_CORRUPTION` to
   `libavb`. Unless the library detects new OS images, this results in a
   `GBL_EFI_AVB_BOOT_COLOR_RED_EIO` flag, requiring user additional confirmation
   before proceeding in degraded mode.
3. `GBL_EFI_AVB_DEVICE_STATUS_UNLOCKED_CRITICAL` - Indicates the device is
   unlocked for critical operations.
4. `GBL_EFI_AVB_DEVICE_STATUS_UNLOCKABLE` - Indicates that the device can be
   unlocked. If the device is not unlockable, calls to
   [`WriteLockState()`][writelockstate] with a _State_ parameter of value
   `GBL_EFI_AVB_LOCK_STATE_UNLOCKED` will fail. See
   [https://source.android.com/docs/core/architecture/bootloader/locking_unlocking].

GBL may call this method multiple times within a single boot session. If the
method returns an error, GBL rejects the boot attempt.

### Status Codes Returned

| Return Code             | Semantics                                              |
| :---------------------- | :----------------------------------------------------- |
| `EFI_SUCCESS`           | A device status is successfully returned.              |
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
enum {
    GBL_EFI_AVB_KEY_VALIDATION_STATUS_INVALID,
    GBL_EFI_AVB_KEY_VALIDATION_STATUS_VALID_CUSTOM_KEY,
    GBL_EFI_AVB_KEY_VALIDATION_STATUS_VALID,
};

typedef uint32_t GBL_EFI_AVB_KEY_VALIDATION_STATUS;
```

##### GBL_EFI_AVB_KEY_VALIDATION_STATUS_INVALID

The public key is not valid. The device cannot continue the boot process for
locked devices; GBL reports a `RED` status and resets. Unlocked devices can
still boot with an `ORANGE` state.

##### GBL_EFI_AVB_KEY_VALIDATION_STATUS_VALID_CUSTOM_KEY

The public key is valid but not fully trusted. GBL continues booting a locked
device with a `YELLOW` state and an unlocked device with an `ORANGE` state.

##### GBL_EFI_AVB_KEY_VALIDATION_STATUS_VALID

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

| Return Code             | Semantics                                              |
| :---------------------- | :----------------------------------------------------- |
| `EFI_SUCCESS`           | Public key validation was successfully completed.      |
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

| Return Code             | Semantics                                                                                 |
| :---------------------- | :---------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`           | The rollback index value is successfully returned.                                        |
| `EFI_NOT_FOUND`         | The requested rollback index isn't supported, so cannot be returned. GBL rejects to boot. |
| `EFI_INVALID_PARAMETER` | Unexpected arguments combination. GBL rejects to boot.                                    |

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

| Return Code             | Semantics                                                                                |
| :---------------------- | :--------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`           | The rollback index value is successfully updated.                                        |
| `EFI_NOT_FOUND`         | The requested rollback index isn't supported, so cannot be updated. GBL rejects to boot. |
| `EFI_INVALID_PARAMETER` | Unexpected arguments combination. GBL rejects to boot.                                   |

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

| Return Code             | Semantics                                                                                                                     |
| :---------------------- | :---------------------------------------------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`           | The requested persistent value is presented and successfully provided in case `Value` buffer isn't NULL.                      |
| `EFI_NOT_FOUND`         | The requested persistent value is not yet populated or supported. GBL will try to initialize it using `WritePersistentValue`. |
| `EFI_BUFFER_TOO_SMALL`  | The provided `Value` buffer is too small. GBL rejects to boot.                                                                |
| `EFI_INVALID_PARAMETER` | Unexpected arguments combination. GBL rejects to boot.                                                                        |

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

| Return Code             | Semantics                                                                                      |
| :---------------------- | :--------------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`           | The value for `Name` is successfully updated.                                                  |
| `EFI_NOT_FOUND`         | Updating the value for `Name` isn't supported. GBL rejects to boot.                            |
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

Note: The dev GBL will attempt to boot using unverified images on an unlocked
device, even after a fatal verification failure and `COLOR_RED` has been
reported to the firmware.

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
   [`ReadPartitionAttributes()`][readpartitionattributes].
4. Display the appropriate UI and obtaining user confirmation for states that
   may affect the device's security guarantees.

Note: The data pointed to by `Result` (including the loaded partitions and
properties buffers) is valid only for the duration of this call and becomes
invalid afterward.

### Status Codes Returned

| Return Code             | Semantics                                                                                          |
| :---------------------- | :------------------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`           | Verification result is successfully handled.                                                       |
| `EFI_INVALID_PARAMETER` | Invalid data is provided by the `Result`. GBL rejects to boot.                                     |
| `EFI_ACCESS_DENIED`     | Failed to update root of trust or other secure world issues occurred. GBL reject the boot attempt. |

## GBL_EFI_AVB_PROTOCOL.WriteLockState()

### Summary

Locks or unlocks the device lock or critical lock.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_AVB_WRITE_LOCK_STATE)(
    IN GBL_EFI_AVB_PROTOCOL *Self,
    IN GBL_EFI_AVB_LOCK_TYPE Type,
    IN GBL_EFI_AVB_LOCK_STATE State,
);
```

### Related Definitions

#### GBL_EFI_AVB_LOCK_TYPE

```c
enum {
    GBL_EFI_AVB_LOCK_TYPE_DEVICE,
    GBL_EFI_AVB_LOCK_TYPE_CRITICAL,
};
typedef uint8_t GBL_EFI_AVB_LOCK_TYPE;
```

##### GBL_EFI_AVB_LOCK_TYPE_DEVICE

The _DEVICE_ lock has the following effects:

| Device lock state | Verification                                   | Boot Color | Fastboot partition read/write/erase |
| :---------------- | :--------------------------------------------- | ---------- | ----------------------------------- |
| Locked            | Enforced, verification failures prevent boot   | Green      | Prohibited                          |
| Unlocked          | Checked, but verification failures are allowed | Orange     | Allowed for non-critical partitions |

Changing the state of the _DEVICE_ lock MUST be preceded by a factory data
reset. GBL guarantees that changing the _DEVICE_ lock with `WriteLockState()`
will always be immediately preceded by erasing non-secure partitions and
performing FDR in this order:

1. Use Block I/O protocols to erase all partitions marked with
   `GBL_EFI_AVB_PARTITION_FLAG_FDR`
2. Call `GBL_EFI_AVB_PROTOCOL.FactoryDataReset()`
3. Call `GBL_EFI_AVB_PROTOCOL.WriteLockState()`

It is security-critical that no user data is allowed to leak across lock state
modifications in either direction. GBL calls `FactoryDataReset()` and
`WriteLockState()` back-to-back, but they will be called from `TPL_APPLICATION`
so are not guaranteed to be atomic if the device could be modifying user data
concurrently (e.g. at a higher TPL or on another core). If there is any chance
of user data modification in-between these calls, the implementation is
responsible for ensuring another FDR is performed atomically with the lock state
update.

Additionally, it is the responsibility of the implementation to display a
relevant UI dialog and obtain user consent before unlocking the device.

##### GBL_EFI_AVB_LOCK_TYPE_CRITICAL

The _CRITICAL_ lock controls read/write/erase access to critical partitions; GBL
will prohibit any fastboot access to these partitions while the _CRITICAL_ lock
is set. Unlike the _DEVICE_ lock, the _CRITICAL_ lock is just a development tool
and has no effect on verification behavior.

The intent of the _CRITICAL_ lock is to provide an extra layer of protection
against unintentionally bricking a device during testing and development. All
partitions required to boot up to GBL fastboot should be guarded by the
_CRITICAL_ lock, with the intent that as long as the _CRITICAL_ lock is enabled
there is no way to permanently brick the device - it can always be recovered by
rebooting into fastboot and flashing the correct OS images. Once the _CRITICAL_
lock is off, this protection is removed.

The set of critical partitions is device-specific and must be provided via
[`ReadPartitionAttributes()`][readpartitionattributes]. If no critical
partitions are specified, the _CRITICAL_ lock has no effect.

Note: the _CRITICAL_ lock is optional. If a firmware implementation does not
support the _CRITICAL_ lock, calls to `WriteLockState()` where `Type` is
`GBL_EFI_AVB_LOCK_TYPE_CRITICAL` should return `EFI_UNSUPPORTED`.

#### GBL_EFI_AVB_LOCK_STATE

```c
enum {
    GBL_EFI_AVB_LOCK_STATE_UNLOCKED,
    GBL_EFI_AVB_LOCK_STATE_LOCKED,
};
typedef uint8_t GBL_EFI_AVB_LOCK_STATE;
```

##### GBL_EFI_AVB_LOCK_STATE_UNLOCKED

Unlock the specified lock.

##### GBL_EFI_AVB_LOCK_STATE_LOCKED

Lock the specified lock.

### Description

Locks or unlocks the _DEVICE_ and _CRITICAL_ locks. See the above definitions
for a description of each lock.

In production devices, these lock states must be stored in secure storage e.g.
RPMB.

Note: The _DEVICE_ and _CRITICAL_ locks are independent, i.e. a device does not
need to prevent (_DEVICE_ == locked, _CRITICAL_ == unlocked). However, the
_CRITICAL_ lock has no effect in this state since the _DEVICE_ lock prohibits
fastboot access to all partitions anyway.

### Status Codes Returned

| Return Code             | Semantics                                                                                    |
| :---------------------- | :------------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`           | The lock state was successfully set.                                                         |
| `EFI_INVALID_PARAMETER` | One of _Type_ or _State_ had an invalid value.                                               |
| `EFI_ACCESS_DENIED`     | The device is not unlockable.                                                                |
| `EFI_UNSUPPORTED`       | _Type_ is `GBL_EFI_AVB_LOCK_TYPE_CRITICAL` and the firmware does not define a critical lock. |

## GBL_EFI_AVB_PROTOCOL.FactoryDataReset()

### Summary

Performs a factory data reset.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_AVB_FACTORY_DATA_RESET)(
    IN GBL_EFI_AVB_PROTOCOL *Self
);
```

### Parameters

#### Self

A pointer to the `GBL_EFI_AVB_PROTOCOL` instance.

### Description

Factory Data Reset (FDR) erases all user data from a device, restoring it to a
fresh out-of-box state.

The implementation is responsible for any secure-world interaction necessary to
perform a secure FDR. This typically involves rotating keys in secure storage
such as RPMB to cryptographically erase user data and protect against replay
attacks.

Implementations may choose to also modify non-secure storage during FDR, but
this cannot be security load-bearing - user data must be permanently deleted
regardless of the state of non-secure storage. If GBL erases any non-secure
partitions itself (via `GBL_EFI_AVB_PARTITION_FLAG_FDR`), these partitions will
be erased prior to the call to `FactoryDataReset()`, so that implementations may
re-initialize these partitions with default contents during FDR if desired.

### Status Codes Returned

| Return Code   | Semantics                                             |
| :------------ | :---------------------------------------------------- |
| `EFI_SUCCESS` | FDR completed and user data has been securely erased. |
| Any error     | FDR failed.                                           |

## Status codes returned to `libavb`

Some of the methods across this protocol are initiated by the `libavb`. The
following UEFI error codes are used to communicate results back to the library:

| Return Code             | Semantics                                                                                                                                               |
| :---------------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `EFI_SUCCESS`           | Requested operation was successful `libavb::AvbIOResult::AVB_IO_RESULT_OK`                                                                              |
| `EFI_OUT_OF_RESOURCES`  | Unable to allocate memory `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_OOM`                                                                                |
| `EFI_DEVICE_ERROR`      | Underlying hardware (disk or other subsystem) encountered an I/O error `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_IO`                                    |
| `EFI_NOT_FOUND`         | Named persistent value or rollback index does not exist for the corresponding key `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_NO_SUCH_VALUE`              |
| `EFI_END_OF_FILE`       | Range of bytes requested to be read or written is outside the range of the partition `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_RANGE_OUTSIDE_PARTITION` |
| `EFI_INVALID_PARAMETER` | Named persistent value size is not supported or does not match the expected size `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_INVALID_VALUE_SIZE`          |
| `EFI_BUFFER_TOO_SMALL`  | Buffer is too small for the requested operation `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_INSUFFICIENT_SPACE`                                           |
| `EFI_UNSUPPORTED`       | Operation isn't implemented / supported                                                                                                                 |
| Others                  | Treated as `libavb::AvbIOResult::AVB_IO_RESULT_ERROR_IO`                                                                                                |

[readpartitionattributes]: #gbl_efi_avb_protocolreadpartitionattributes
[readdevicestatus]: #gbl_efi_avb_protocolreaddevicestatus
[handleverificationresult]: #gbl_efi_avb_protocolhandleverificationresult
[protocolwriterollbackindex]: #gbl_efi_avb_protocolwriterollbackindex
[validatevbmetapublickey]: #gbl_efi_avb_protocolvalidatevbmetapublickey
[readrollbackindex]: #gbl_efi_avb_protocolreadrollbackindex
[writerollbackindex]: #gbl_efi_avb_protocolwriterollbackindex
[readpersistentvalue]: #gbl_efi_avb_protocolreadpersistentvalue
[writepersistentvalue]: #gbl_efi_avb_protocolwritepersistentvalue
[handleverificationresult]: #gbl_efi_avb_protocolhandleverificationresult
[writelockstate]: #gbl_efi_avb_protocolwritelockstate
[factorydatareset]: #gbl_efi_avb_protocolfactorydatareset
[avb]: https://source.android.com/docs/security/features/verifiedboot/avb
[unlocked]:
  https://android.googlesource.com/platform/external/avb/+/refs/heads/main/README.md#locked-and-unlocked-mode
[oem_unlocking]:
  https://source.android.com/docs/core/architecture/bootloader/locking_unlocking
[dmv_error]:
  https://android.googlesource.com/platform/external/avb/+/master/README.md#handling-dm_verity-errors
[rp]:
  https://android.googlesource.com/platform/external/avb/+/android16-release/README.md#rollback-protection
[update_ri]:
  https://android.googlesource.com/platform/external/avb/+/android16-release/README.md#updating-stored-rollback-indexes
[pd]:
  https://android.googlesource.com/platform/external/avb/+/android16-release/README.md#persistent-digests
[boot_flow]:
  https://source.android.com/docs/security/features/verifiedboot/boot-flow
[boot_flow_red]:
  https://source.android.com/docs/security/features/verifiedboot/boot-flow#no-valid-os-found
[boot_flow_orange]:
  https://source.android.com/docs/security/features/verifiedboot/boot-flow#unlocked-devices
[boot_flow_yellow]:
  https://source.android.com/docs/security/features/verifiedboot/boot-flow#locked-devices-with-custom-root-of-trust
[boot_flow_red_eio]:
  https://source.android.com/docs/security/features/verifiedboot/boot-flow#dm-verity-corruption
