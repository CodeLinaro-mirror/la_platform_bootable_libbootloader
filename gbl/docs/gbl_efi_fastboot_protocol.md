# GBL EFI Fastboot Protocol

This document describes the GBL Fastboot protocol. The protocol defines
interfaces that can be used by EFI applications to query and modify vendor-specific
information on a device that may be desired in the context of a fastboot environment.

|             |                    |
|:------------|-------------------:|
| **Status**  | *Work in progress* |
| **Created** |          2024-9-11 |

## `GBL_EFI_FASTBOOT_PROTOCOL`

### Summary

This protocol provides interfaces for platform-specific operations during Fastboot.
This can include support for vendor defined variables or variables whose query
requires cooperation with vendor firmware, OEM commands,

### GUID
```c
// {c67e48a0-5eb8-4127-be89-df2ed93d8a9a}
#define GBL_EFI_FASTBOOT_PROTOCOL_GUID               \
  {                                                  \
    0xc67e48a0, 0x5eb8, 0x4127, {                    \
      0xbe, 0x89, 0xdf, 0x2e, 0xd9, 0x3d, 0x8a, 0x9a \
    }                                                \
  }
```

### Revision Number

```c
#define GBL_EFI_FASTBOOT_PROTOCOL_REVISION GBL_PROTOCOL_REVISION(0, 1)
```

See [GBL Custom Protocol Revisions](efi_protocols.md#gbl-custom-protocol-revisions) for details about protocol revisions.

### Protocol Interface Structure

```c
#define GBL_EFI_FASTBOOT_SERIAL_NUMBER_MAX_LEN_UTF8 32

typedef struct _GBL_EFI_FASTBOOT_PROTOCOL {
  UINT64                                        Revision
  CHAR8                                         SerialNumber[GBL_EFI_FASTBOOT_SERIAL_NUMBER_MAX_LEN_UTF8];
  GBL_EFI_FASTBOOT_GET_VAR                      GetVar;
  GBL_EFI_FASTBOOT_GET_VAR_ALL                  GetVarAll;
  GBL_EFI_FASTBOOT_RUN_OEM_FUNCTION             RunOemFunction;
  GBL_EFI_FASTBOOT_GET_STAGED                   GetStaged;
  GBL_EFI_FASTBOOT_GET_POLICY                   GetPolicy;
  GBL_EFI_FASTBOOT_SET_LOCK                     SetLock;
  GBL_EFI_FASTBOOT_GET_LOCK                     GetLock;
  VOID*                                         Reserved[3];
  GBL_EFI_FASTBOOT_GET_PARTITION_PERMISSIONS    GetPartitionPermissions;
  GBL_EFI_FASTBOOT_WIPE_USER_DATA               WipeUserData;
  GBL_EFI_FASTBOOT_SHOULD_STOP_IN_FASTBOOT      ShouldStopInFastboot;
} GBL_EFI_FASTBOOT_PROTOCOL;
```

### Parameters

**Revision**

The revision to which the `GBL_EFI_FASTBOOT_PROTOCOL` adheres.
All future revisions must be backwards compatible.
If a future version is not backwards compatible, a different GUID must be used.

**SerialNumber**

The device serial number expressed as a Null-terminated UTF-8 encoded string.
If the device serial number is 32 bytes long, the Null terminator must be excluded.
If the device serial number is longer than 32 bytes, it must be truncated.

**GetVar**

Gets the value for the given fastboot variable.
See [`GBL_EFI_FASTBOOT_PROTOCOL.GetVar()`](#gbl_efi_fastboot_protocolgetvar).

**GetVarAll**

Iterates all combinations of arguments and values for all fastboot variables.
See [`GBL_EFI_FASTBOOT_PROTOCOL.GetVarAll()`](#gbl_efi_fastboot_protocolgetvarall).

**RunOemFunction**

Runs an OEM-defined command on the device.
See [`GBL_EFI_FASTBOOT_PROTOCOL.RunOemFunction()`](#gbl_efi_fastboot_protocolrunoemfunction).

**GetStaged**

Read OEM provided payload for uploading to fastboot host by command
`fastboot get_staged`. See
[`GBL_EFI_FASTBOOT_PROTOCOL.GetStaged()`](#gbl_efi_fastboot_protocolgetstaged).

**GetPolicy**

Querys device policy including device lock state, whether the device firmware
supports a 'critical' lock, and whether the device is capable of booting from
an image loaded directly into RAM.
See [`GBL_EFI_FASTBOOT_PROTOCOL.GetPolicy()`](#gbl_efi_fastboot_protocolgetpolicy).

**SetLock**

Locks or unlocks device or critical partitions.
See [`GBL_EFI_FASTBOOT_PROTOCOL.SetLock()`](#gbl_efi_fastboot_protocolsetlock).

**GetLock**

Queries lock status of device or critical partitions.
See [`GBL_EFI_FASTBOOT_PROTOCOL.GetLock()`](#gbl_efi_fastboot_protocolGetLock).

**GetPartitionPermissions**

Queries permissions information about the provided partition.
See [`GBL_EFI_FASTBOOT_PROTOCOL.GetPartitionPermissions()`](#gbl_efi_fastboot_protocolgetpartitionpermissions).

**WipeUserData**

Erases all partitions containing user data.
See [`GBL_EFI_FASTBOOT_PROTOCOL.WipeUserData()`](#gbl_efi_fastboot_protocolwipeuserdata).

## `GBL_EFI_FASTBOOT_PROTOCOL.GetVar()`

### Summary

Gets the value for a fastboot variable.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_GET_VAR)(
    IN GBL_EFI_FASTBOOT_PROTOCOL*         This,
    IN CONST CHAR8* CONST*                Args,
    IN UINTN                              NumArgs,
    OUT CHAR8*                            Buf,
    IN OUT UINTN*                         BufSize,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure)
instance.

*Args*

A pointer to an array of NULL-terminated strings that contains the name of the
variable followed by additional arguments.

*NumArgs*

The number of elements in the *Args* array.

*Buf*

A pointer to the data buffer to store the value of the variable as a UTF-8
encoded string.

*BufSize*

On entry, the size in bytes of *Buf*.
On exit, the size in bytes of the UTF-8 encoded string describing the value,
excluding any Null-terminator.

### Description

`GetVar()` queries internal data structures and drivers to determine the value
of the given variable. Variables may have zero or more additional arguments.
These arguments are parsed by the caller and passed to `GetVar()` as an array
of NULL-terminated UTF-8 encoded string.

An example client interaction:
```bash
# A variable with no argument.
$ fastboot getvar max-download-size
OKAY0x20000000

# A variable with two arguments.
$ fastboot getvar block-device:0:total-blocks
OKAY0x800000000000
```

### Status Codes Returned

| Return Code             | Semantics                                                                                                                                                                |
|:------------------------|:-------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `EFI_SUCCESS`           | The variable was found and its value successfully serialized.                                                                                                            |
| `EFI_INVALID_PARAMETER` | One of *This*, *Args*, *Buf*, or *BufSize* is `NULL`                                                                                                                     |
| `EFI_NOT_FOUND`         | The first element of *Args* does not contain a known variable.                                                                                                           |
| `EFI_UNSUPPORTED`       | The contents of *Args* do not contain a known variable with valid aruments. Any of the subarguments may be unknown, or too many or too few subarguments may be provided. |
| `EFI_BUFFER_TOO_SMALL`  | *Buf* is too small to store the serialized variable string. The value of *BufSize* is modified to contain the minimum necessary buffer size.                             |

## `GBL_EFI_FASTBOOT_PROTOCOL.GetVarAll()`

### Summary

Iterates all combinations of variables and values.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_GET_VAR_ALL)(
    IN GBL_EFI_FASTBOOT_PROTOCOL*         This,
    IN VOID*                              Context
    IN GBL_EFI_GET_VAR_ALL_CALLBACK       GetVarAllCallback,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure)
instance.

*Context*

A pointer to the context data for `GetVarAllCallback`.

*GetVarAllCallback*

A pointer to a function of type `GBL_EFI_GET_VAR_ALL_CALLBACK`. It receives as
parameter the `Context` pointer passed to this function, an array of
NULL-terminated UTF8 strings containing variable name and additional arguments,
the array length, and a NULL-terminated string representing the value.

### Related Definitions

```c
typedef
VOID (*GBL_EFI_GET_VAR_ALL_CALLBACK) (
    IN VOID*                              Context
    IN CONST CHAR8* CONST*                Args,
    IN UINTN                              NumArgs,
    IN CONST CHAR8*                       Value,
);
```
*Context*

The pointer to the context passed to `GetVarAll()`.

*Args*

A pointer to an array of NULL-terminated strings that contains the name of the
variable followed by additional arguments.

*NumArgs*

The number of elements in the *Args* array.

*Value*

A NULL-terminated string representing the value.

### Description

`GetVarAll()` iterates all combinations of arguments and values for all fastboot
variables. For each combination, the function invokes the caller provided
callback `GetVarAllCallback()` and passes the context, arguments and value.

### Status Codes Returned

| Return Code             | Semantics                                       |
|:------------------------|:------------------------------------------------|
| `EFI_SUCCESS`           | Operation is successful.                        |
| `EFI_INVALID_PARAMETER` | One of *This* or *GetVarAllCallback* is `NULL`. |

## `GBL_EFI_FASTBOOT_PROTOCOL.RunOemFunction()`

### Summary

Runs a vendor defined function that requires firmware support.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_RUN_OEM_FUNCTION)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* This,
    IN CHAR8*                     Command,
    IN UINTN                      CommandLen,
    IN UINT8*                     DownloadData,
    IN UINTN                      DownloadDataLen,
    IN FASTBOOT_MESSAGE_SENDER    Sender,
    IN VOID*                      SenderContext,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure) instance.

*Command*

The command to run as a UTF-8 encoded string, excluding the "oem " prefix.
The string does not need to be NULL terminated.

*CommandLen*

The length of the command in bytes, excluding any Null-terminator.

*DownloadData*

A pointer to the most recent download data.

*DownloadDataLen*

The size of the download data in `DownloadData`.

`DownloadData` and `DownloadDataLen` can be used to implement OEM commands that
process platform specific payload. i.e.
`fastboot stage <data> && fastboot oem process-data`,

*Sender*

A pointer to a function of type `FASTBOOT_MESSAGE_SENDER`. The function is used
by the implementation to send custom fastboot OKAY/FAIL/INFO messages. For input
arguments, it takes the `SenderContext` pointer passed to this function, the
message type, a pointer to a UTF8 string and the string length.

OKAY/FAIL messages should only be sent once. Sending it multiple times in
a single command may break fastboot exahcange sequence. Caller that provides
`FASTBOOT_MESSAGE_SENDER` should check this situation and return
`EFI_PROTOCOL_ERROR` if implementation attempts to send a OKAY/FAIL more than
once.

Likewise if implementation returns without sending any OKAY/FAIL message, caller
should send a default one based on the return value of this API.

*SenderContext*

A pointer to the context data for `Sender`.

### Description

`RunOemFunction()` runs a vendor defined Oem function. These functions can take
arbitrary arguments or subcommands. The caller does no parsing or verification.
All parsing and verification is the responsibility of the method
implementation. Oem functions can display power or battery information, print
or iterate over UEFI variables, or conduct arbitrary other operations.

Implementation may choose not to return from the function and take over the
control flow. This can be the case for oem commands that implements platform
specific reboot or side loading/booting of platform specific payload. However,
in this case, implementation should make sure to send a OKAY or FAIL message
using `Sender` to prevent host from hanging waiting for reply.

### Related Definitions

```c
typedef enum EFI_FASTBOOT_MESSAGE_TYPE {
  OKAY,
  FAIL,
  INFO,
} EFI_FASTBOOT_MESSAGE_TYPE;

typedef
EFI_STATUS (*FASTBOOT_MESSAGE_SENDER) (
    IN VOID*                      Context,
    IN EFI_FASTBOOT_MESSAGE_TYPE  MsgType
    IN CONST CHAR8*               Msg,
    IN UINTN                      Len,
);
```
*Context*

The pointer to the context passed to `RunOemFunction()`.

*MsgType*

A `EFI_FASTBOOT_MESSAGE_TYPE` value indicating message type.

*Msg*

A pointer to a UTF8 string. The string does not need to be NULL terminated.

*Len*

The length of `Msg`.

Note: The max allowed length of a message depends on the transport. For
example, for Fastboot over USB, it is the native packet size. Implementation
should consider the transport setup it provides when passing the string.
Oversized message may be truncated by the caller when sent to the host.


### Status Codes Returned

| Return Code             | Semantics
|:------------------------|:---------------------------------------------------------|
| `EFI_SUCCESS`           | The call completed successfully.                         |
| `EFI_INVALID_PARAMETER` | Any of *This*, *Command*, *Sender* is `NULL`.            |
| `EFI_NOT_FOUND`         | The command is not supported.                            |
| `EFI_ACCESS_DENIED`     | The operation is not permitted in the current lock state.|


## `GBL_EFI_FASTBOOT_PROTOCOL.GetStaged()`

### Summary

Read OEM provided payload for uploading to the host during command
`fastboot get_staged`.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_GET_STAGED)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* This,
    IN UINT8*                     Out,
    IN OUT UINTN*                 OutLen,
    OUT UINTN*                    RemainingSize,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure) instance.

*Out*

Pointer to the output buffer.

*OutLen*

On input, stores the size of the output buffer `Out`. On output, stores the
actual number of bytes read to `Out`.

*RemainingSize*

On output, stores the number of remaining bytes left to read.

### Description

`GetStaged()` reads OEM defined data for uploading to fastboot host during
command `fastboot get_staged`. The function may be called multiple times to
read out the whole payload in chunks to accommodate callers with limited buffer.
Implementation should internally track read progress and avoid changing the
backing data when caller starts reading. However, outside the session of
`fastboot get_staged`, i.e. when in `RunOemFunction`, implementation can change
or update the backing data.

Caller may pass a 0-length input buffer for peeking the total via
`RemainingSize`. This should be expected by the implementation.

The typical usage is to for vendor to provide an OEM command that sets up the
payload and then retrieve the payload via `fastboot get_staged` from the host.

### Status Codes Returned

| Return Code             | Semantics
|:------------------------|:---------------------------------------------------------|
| `EFI_SUCCESS`           | The call completed successfully.                         |
| `EFI_INVALID_PARAMETER` | Any of *Out*, *OutLen*, *RemainingSize* is `NULL`.       |
| `EFI_ACCESS_DENIED`     | The operation is not permitted in the current lock state.|


## `GBL_EFI_FASTBOOT_PROTOCOL.GetPolicy()`

### Summary

Gets the device policy pertaining to locking and booting directly from RAM.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_GET_POLICY)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* This,
    OUT GBL_EFI_FASTBOOT_POLICY*  Policy,
);
```

### Related Definitions

```c
typedef struct _GBL_EFI_FASTBOOT_POLICY {
  // Indicates whether device can be unlocked.
  BOOL CanUnlock;
  // Device firmware supports 'critical' partition locking.
  BOOL HasCriticalLock;
  // Indicates whether device allows booting
  // from images loaded directly from RAM.
  BOOL CanRamBoot;
} GBL_EFI_FASTBOOT_POLICY;

```

### Parameters

*This*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure) instance.

*Policy*

On exit contains the device policy.
See [Related Definitions](#related-definitions-2) for the definition of `GBL_EFI_FASTBOOT_POLICY`.

### Description

Depending on various factors including whether the device
is a development target or end-user device,
certain operations may be prohibited.
In particular, loading an image directly into RAM and then booting it
is generally not permitted on anything except development hardware.
Developer workflows and CI/CD infrastructure need to be able to query
whether a device is able to be unlocked and whether RAM booting is permitted.

See [`SetLock()`](#gbl_efi_fastboot_protocolsetlock) for a method that modifies
the device lock state.

### Status Codes

| Return Code             | Semantics                                                  |
|:------------------------|:-----------------------------------------------------------|
| `EFI_SUCCESS`           | The device policy was successfuly retrieved.               |
| `EFI_INVALID_PARAMETER` | One of *This* or *Policy* is `NULL` or improperly aligned. |

## `GBL_EFI_FASTBOOT_PROTOCOL.SetLock()`

### Summary

Sets device partition locks.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_SET_LOCK)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* This,
    IN BOOL                       Critical,
    IN BOOL                       Lock,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure) instance.

*Critical*

Set to true if operation is to lock/unlock critical partitions. Set to false if
operation is to lock/unlock device.

*Lock*

Set to true to lock. Set to false to unlock.

### Description

Device lock state determines what operations can be performed on device partitions.
`SetLock()` locks or unlocks device or critical partitions.

### Status Codes Returned

| Return Code             | Semantics                                          |
|:------------------------|:---------------------------------------------------|
| `EFI_SUCCESS`           | The call completed successfully.                   |
| `EFI_INVALID_PARAMETER` | *This* is invalid or improperly aligned.           |
| `EFI_ACCESS_DENIED`     | Caller intends to lock/unlock device or critical partition but device prohibits the operation. |

## `GBL_EFI_FASTBOOT_PROTOCOL.GetLock()`

### Summary

Qeury lock status.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_GET_LOCK)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* This,
    IN BOOL                       Critical,
    OUT BOOL                      *Lock,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure) instance.

*Critical*

Set to true to query lock/unlock status of critical partitions. Set to false to
query lock/unlock status of device.

*Lock*

Stores the output lock status. Set to true if status is locked. Set to false
otherwise.

### Description

`GetLock()` queries the lock status of device or critical partitions.

### Status Codes Returned

| Return Code             | Semantics                                          |
|:------------------------|:---------------------------------------------------|
| `EFI_SUCCESS`           | The call completed successfully.                   |
| `EFI_INVALID_PARAMETER` | *This* is invalid or improperly aligned.           |

## `GBL_EFI_FASTBOOT_PROTOCOL.GetPartitionPermissions()`

### Summary

Gets access permission information about the given partition.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_GET_PARTITION_PERMISSIONS)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* This,
    IN CHAR8*                     PartName,
    IN UINTN                      PartNameLen,
    OUT UINT64                    Permissions,
);
```

### Related Definitions

```c
typedef enum _GBL_EFI_FASTBOOT_PARTITION_PERMISSION_FLAGS {
  // Firmware can read the given partition and send its data to fastboot client.
  GBL_EFI_FASTBOOT_PARTITION_READ = 0x1 << 0,
  // Firmware can overwrite the given partition.
  GBL_EFI_FASTBOOT_PARTITION_WRITE = 0x1 << 1,
  // Firmware can erase the given partition.
  GBL_EFI_FASTBOOT_PARTITION_ERASE = 0x1 << 2,
} GBL_EFI_FASTBOOT_PARTITION_PERMISSION_FLAGS;

```

### Parameters

*This*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure) instance.

*PartName*

The name of the partition to query as a UTF-8 encoded, Null-terminated string.

*PartNameLen*

The length of *PartName* in bytes, excluding any Null-terminator.

*Permissions*

On exit contains the ORed flags detailing the current fastboot permissions for
the given partition.
See [Related Definitions](#related-definitions-4) for flag value semantics.

### Description

Depending on device lock state, Android Verified Boot policy, and other factors,
various partitions may have restricted permissions within a fastboot environment.
`GetPartitionPermissions()` retrieves the current permissions
for the requested partition.

By default, unless overridden by device policy, no operations are permitted on
any partition when the device is locked, and all operations are permitted
on all partitions when the device is unlocked.

### Status Codes

| Return Code             | Semantics                                                                          |
|:------------------------|:-----------------------------------------------------------------------------------|
| `EFI_SUCCESS`           | The partition permision information was successfully queried.                      |
| `EFI_INVALID_PARAMETER` | One of *This*, *PartName*, or *Permissions* is `NULL` or improperly aligned.       |
| `EFI_NOT_FOUND`         | There is no partition named *PartName*.                                            |
| `EFI_UNSUPPORTED`       | The device does not have a partition permission policy different from the default. |

## `GBL_EFI_FASTBOOT_PROTOCOL.WipeUserData()`

### Summary

Erases all partitions containing user data.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_WIPE_USER_DATA)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* This,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure) instance.

### Description

Device user data is often stored on a dedicated partition
apart from kernel images or other system data.
This helps protect user data during system upgrades.
`WipeUserData()` erases all user data partitions.
This can be used to restore a device to its factory settings,
as part of a refurbishment process, or for testing purposes.

### Status Codes

| Return Code             | Semantics                                                 |
|:------------------------|:----------------------------------------------------------|
| `EFI_SUCCESS`           | User data was successfully wiped.                         |
| `EFI_INVALID_PARAMETER` | *This* is `NULL` or improperly aligned.                   |
| `EFI_ACCESS_DENIED`     | The operation is not permitted in the current lock state. |
| `EFI_DEVICE_ERROR`      | There was a block device or storage error.                |

## `GBL_EFI_FASTBOOT_PROTOCOL.ShouldStopInFastboot()`

### Summary

Checks custom inputs to determine whether the device should stop in fastboot on boot.

### Prototype

```c
typedef
BOOL
(EFIAPI * GBL_EFI_FASTBOOT_SHOULD_STOP_IN_FASTBOOT)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* This,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure) instance.

### Description

Devices often define custom mechanisms for determining whether to enter fastboot mode
on boot. A specific button press combination is common,
e.g. pressing 'volume down' for three seconds while booting.

`ShouldStopInFastboot()` returns whether the device should stop in fastboot mode
due to device input.

**Note:** `ShouldStopInFastboot()` should ONLY return `true` if the device specific
button press is active. In particular, if the device supports
[`GBL_EFI_AB_SLOT_PROTOCOL`](./gbl_efi_ab_slot_protocol.md),
`ShouldStopInFastboot()` should NOT check the information provided by
`GBL_EFI_AB_SLOT_PROTOCOL.GetBootReason()` or the underlying persistent boot reason.

Any errors should cause a return value of `false`.
