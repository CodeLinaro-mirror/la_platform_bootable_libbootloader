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
  GBL_EFI_FASTBOOT_SET_LOCK                     SetLock;
  GBL_EFI_FASTBOOT_GET_LOCK                     GetLock;
  GBL_EFI_FASTBOOT_VENDOR_ERASE                 VendorErase;
  GBL_EFI_FASTBOOT_SHOULD_STOP_IN_FASTBOOT      ShouldStopInFastboot;
  GBL_EFI_FASTBOOT_IS_COMMAND_ALLOWED           IsCommandAllowed;
  GBL_EFI_FASTBOOT_START_LOCAL_SESSION          StartLocalSession;
  GBL_EFI_FASTBOOT_UPDATE_LOCAL_SESSION         UpdateLocalSession;
  GBL_EFI_FASTBOOT_CLOSE_LOCAL_SESSION          CloseLocalSession;
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

**SetLock**

Locks or unlocks device or critical partitions.
See [`GBL_EFI_FASTBOOT_PROTOCOL.SetLock()`](#gbl_efi_fastboot_protocolsetlock).

**GetLock**

Queries lock status of device or critical partitions.
See [`GBL_EFI_FASTBOOT_PROTOCOL.GetLock()`](#gbl_efi_fastboot_protocolGetLock).

**VendorErase**

Performs vendor specific erase for a partition during handling of
`fastboot erase <partition>`
See [`GBL_EFI_FASTBOOT_PROTOCOL.VendorErase()`](#gbl_efi_fastboot_protocolvendorerase).

**ShouldStopInFastboot**

Checks whether boot should stop in fastboot mode. See
[`GBL_EFI_FASTBOOT_PROTOCOL.ShouldStopInFastboot()`](#gbl_efi_fastboot_protocolshouldstopinfastboot)

**IsCommandAllowed**

Checks if a fastboot command is allowed by the platform.
See [`GBL_EFI_FASTBOOT_PROTOCOL.IsCommandAllowed()`](#gbl_efi_fastboot_protocoliscommandallowed).

**StartLocalSession**

Starts a local fastboot session driven by UI, usually outputting to a screen
and navigated using buttons.
See [`GBL_EFI_FASTBOOT_PROTOCOL.StartLocalSession()`](#gbl_efi_fastboot_protocolstartlocalsession).

**UpdateLocalSession**

Updates the local fastboot session UI as part of a polling loop.
The local fastboot UI should update the screen, accept button input,
and return fastboot commands for GBL to handle.
See [`GBL_EFI_FASTBOOT_PROTOCOL.UpdateLocalSession()`](#gbl_efi_fastboot_protocolupdatelocalsession).

**CloseLocalSession**

Terminates the local session and conducts any necessary cleanup.
GBL will call this method before any reboot, boot, or continue command from any fastboot session.
See [`GBL_EFI_FASTBOOT_PROTOCOL.CloseLocalSession()`](#gbl_efi_fastboot_protocolcloselocalsession).


## `GBL_EFI_FASTBOOT_PROTOCOL.GetVar()`

### Summary

Gets the value for a fastboot variable.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_GET_VAR)(
    IN GBL_EFI_FASTBOOT_PROTOCOL*         Self,
    IN CONST CHAR8* CONST*                Args,
    IN UINTN                              NumArgs,
    OUT CHAR8*                            Buf,
    IN OUT UINTN*                         BufSize,
);
```

### Parameters

*Self*

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
| `EFI_INVALID_PARAMETER` | One of *Self*, *Args*, *Buf*, or *BufSize* is `NULL`                                                                                                                     |
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
    IN GBL_EFI_FASTBOOT_PROTOCOL*         Self,
    IN VOID*                              Context
    IN GBL_EFI_GET_VAR_ALL_CALLBACK       GetVarAllCallback,
);
```

### Parameters

*Self*

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

The name and arguments correspond to the  `:` separated variable format by
the fastboot protocol, i.e. `fastboot getvar <name>:<arg1>:<arg2>..`. However
firmware may also choose to pass the entire `"<name>:<arg1>:<arg2>.."` string
as a 1-size array if preferred. Caller should expect both cases.


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
| `EFI_INVALID_PARAMETER` | One of *Self* or *GetVarAllCallback* is `NULL`. |

## `GBL_EFI_FASTBOOT_PROTOCOL.RunOemFunction()`

### Summary

Runs a vendor defined function that requires firmware support.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_RUN_OEM_FUNCTION)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* Self,
    IN CHAR8*                     Command,
    IN UINTN                      CommandLen,
    IN UINT8*                     DownloadData,
    IN UINTN                      DownloadDataLen,
    IN FASTBOOT_MESSAGE_SENDER    Sender,
    IN VOID*                      SenderContext,
);
```

### Parameters

*Self*

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
| `EFI_INVALID_PARAMETER` | Any of *Self*, *Command*, *Sender* is `NULL`.            |
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
    IN GBL_EFI_FASTBOOT_PROTOCOL* Self,
    IN UINT8*                     Out,
    IN OUT UINTN*                 OutLen,
    OUT UINTN*                    RemainingSize,
);
```

### Parameters

*Self*

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

## `GBL_EFI_FASTBOOT_PROTOCOL.SetLock()`

### Summary

Sets device partition locks.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_SET_LOCK)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* Self,
    IN BOOL                       Critical,
    IN BOOL                       Lock,
);
```

### Parameters

*Self*

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
| `EFI_INVALID_PARAMETER` | *Self* is invalid or improperly aligned.           |
| `EFI_ACCESS_DENIED`     | Caller intends to lock/unlock device or critical partition but device prohibits the operation. |

## `GBL_EFI_FASTBOOT_PROTOCOL.GetLock()`

### Summary

Qeury lock status.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_GET_LOCK)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* Self,
    IN BOOL                       Critical,
    OUT BOOL                      *Lock,
);
```

### Parameters

*Self*

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
| `EFI_INVALID_PARAMETER` | *Self* is invalid or improperly aligned.           |
| `EFI_UNSUPPORTED`       | The corresponding lock is unsupported. |

## `GBL_EFI_FASTBOOT_PROTOCOL.VendorErase()`

### Summary

Performs vendor specific erase for a partition.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_VENDOR_ERASE)(
    IN GBL_EFI_FASTBOOT_PROTOCOL*       Self,
    IN CHAR8*                           PartName,
    IN UINTN                            PartNameLen,
    OUT GBL_EFI_FASTBOOT_ERASE_ACTION   *Action,
);
```

### Parameters

*Self*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure) instance.

*PartName*

The name of the partition to query as a UTF-8 encoded, Null-terminated string.
This should be the same partition name passed from
`fastboot erase <partition>`.

*PartNameLen*

The length of *PartName* in bytes, excluding any Null-terminator.

*Action*

On exit, stores the action for the caller to perform. See definition of
`GBL_EFI_FASTBOOT_ERASE_ACTION`.


### Description

The API is for firmware to implement vendor specific erase logic during
handling of `fastboot erase <partition>`. This can be used for partiitons that
are virtual (i.e. metadata, cache) and partitions whose erase requires side
effect such as resetting of metadata stored somewhere else.

On exit, the API can suggest actions caller should take. If firmware wants the
caller to treat the partition as a regular on-disk partition and perform a
normal erase, `Action` should be set to `ERASE_AS_PHYSICAL_PARTITION`. If
firmware has performed all necessary erase work and caller doesn't need to do
anything, `Action` should be set to `NOOP`.

### Related Definitions

```c
typedef enum  {
  // Treats the partition as a physical partition on disk and erases it.
  ERASE_AS_PHYSICAL_PARTITION,
  // Ignores the partition.
  NOOP,
} GBL_EFI_FASTBOOT_ERASE_ACTION;
```

### Status Codes

| Return Code             | Semantics |
|:------------------------|:-|
| `EFI_SUCCESS`           | The partition permision information was successfully queried. |
| `EFI_INVALID_PARAMETER` | *PartName* or *Action* is `NULL`. |
| `EFI_DEVICE_ERROR` | An internal device error occurred. |

## `GBL_EFI_FASTBOOT_PROTOCOL.ShouldStopInFastboot()`

### Summary

Checks custom inputs to determine whether the device should stop in fastboot on boot.

### Prototype

```c
typedef
BOOL
(EFIAPI * GBL_EFI_FASTBOOT_SHOULD_STOP_IN_FASTBOOT)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* Self,
);
```

### Parameters

*Self*

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

## `GBL_EFI_FASTBOOT_PROTOCOL.IsCommandAllowed()`

### Summary

Checks whether a fastboot command is allowed.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_IS_COMMAND_ALLOWED)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* Self,
    IN UINTN                      NumArgs,
    IN CONST CHAR8* CONST*        Args,
    IN UINTN                      DownloadDataLen,
    IN UINT8*                     DownloadData,
    OUT BOOLEAN                   *Allowed,
    IN UINTN                      MsgBufSize,
    OUT CHAR8*                    MsgBuf,
);
```

### Parameters

*Self*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure) instance.

*NumArgs*

The number of elements in the *Args* array.

*Args*

A pointer to an array of NULL-terminated UTF-8 strings that contains the
fastboot command followed by additional arguments.

*DownloadData*

A pointer to the most recent downloaded data.

*DownloadDataLen*

The size of the download data in `DownloadData`.

`DownloadData` and `DownloadDataLen` provide additional context for commands
such as `fastboot flash`.

*Allowed*

On exit, set to TRUE if the command is allowed. Set to FALSE otherwise.

*MsgBufSize*

Store the size of `MsgBuf`.

*MsgBuf*

On exit, stores a NULL-terminated UTF-8 output message.

### Description

`IsCommandAllowed()` queries whether a fastboot command is allowed by the
platform. When command is not allowed, firmware can output an optional
NULL-terminated message in `MsgBuf`.

It's up to the caller to decide how to proceed in the case of error, i.e base
on the level of security requirement.

### Status Codes Returned

| Return Code             | Semantics                                          |
|:------------------------|:---------------------------------------------------|
| `EFI_SUCCESS`           | The call completed successfully.                   |
| `EFI_INVALID_PARAMETER` | `Command` or `Allowed` or `MsgBuf` is NULL. `DownloadDataLen` is non-zero but `DownloadData` is NULL. |
| `EFI_DEVICE_ERROR`      | An internal error occurred. |

## `GBL_EFI_FASTBOOT_PROTOCOL.StartLocalSession()`

### Summary

Starts a local fastboot session UI.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_START_LOCAL_SESSION)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* Self,
    OUT VOID**                    Ctx);
```

### Parameters

*Self*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure)
instance.

*Ctx*

Pointer to saved Context for the local session.

### Description

Devices with screens and input buttons may wish to provide a local bootloader menu during Fastboot to allow user control without requiring an attached controller.
Starts the local boot menu or indicates that the local boot menu is not supported or is not necessary.

### Status Codes Returned
| Return Code             | Semantics                                                       |
|:------------------------|:----------------------------------------------------------------|
| `EFI_SUCCESS`           | The call completed successfully and the context saved to *Ctx*. |
| `EFI_INVALID_PARAMETER` | One of *Self* or *Ctx* is `NULL`.                               |
| `EFI_UNSUPPORTED`       | The device does not support a local boot menu.                  |

## `GBL_EFI_FASTBOOT_PROTOCOL.UpdateLocalSession()`

### Summary

Polls the local session to update the display, check for input, and generate fastboot
commands based on input.

### Prototype

```c
EFI_STATUS (EFIAPI * GBL_EFI_UPDATE_LOCAL_SESSION)(
     IN GBL_EFI_FASTBOOT_PROTOCOL* Self,
     IN VOID* Ctx,
     OUT UINT8* Buf,
     IN OUT UINTN* BufSize,
);
```

### Parameters

*Self*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure)
instance.

*Ctx*

A pointer to the local session context provided by a call to `StartLocalSession()`.

*Buf*

A pointer to the data buffer to store the value of a fastboot command GBL should
evaluate encoded as a UTF-8 string.

*BufSize*

On entry, the size in bytes of *Buf*.
On exit, the size in bytes of the UTF-8 encoded string describing the fastboot command
stored in *Buf* excluding any Null-terminator.
May be `0` on exit if no command is requested.

### Description

This method drives the local session and updates its context.

Once the local session has started, GBL **SHOULD** wait no more than 1 millisecond
before calling `UpdateLocalSession` or `CloseLocalSession`. GBL **SHOULD** poll
`UpdateLocalSession` with a period of no less than 1 millisecond before calling
`CloseLocalSession`.

The 1 millisecond delay is a best effort attempt.
The delay may be larger, and there may be jitter.

Warning: the local boot menu may run concurrently with network or USB fastboot sessions.
Calls to `UpdateLocalSession` **MUST NOT** do any of the following:
* initiate calls to blocking I/O
* mutate global state without acquiring a relevant mutex
* modify persistent state, i.e. block storage or persistent registers
* reboot or power off the device

Between polls, GBL may do any of the following:
* conduct non-blocking I/O
* handle fastboot commands sent via USB or the network
* run oem custom functions
* call UEFI boot service methods

The local session can request that GBL take certain actions, e.g. reboot the device
or erase partitions, by formulating fastboot commands in the return buffer.

Logic in `UpdateLocalSession` **SHOULD** refrain from heavy computation or any other
operation that may take more than ~100μs.

### Status Codes Returned

| Return Code           | Semantics                                                                                                                                              |
|:----------------------|:-------------------------------------------------------------------------------------------------------------------------------------------------------|
| EFI_SUCCESS           | The call completed successfully.                                                                                                                       |
| EFI_INVALID_PARAMETER | One of *Self*, *Ctx*, *Buf*, or *BufSize* is `NULL`.                                                                                                   |
| EFI_BUFFER_TOO_SMALL  | The provided buffer is to small to store the output fastboot command. The value of *BufSize* is modified to contain the minimum necessary buffer size. |
| EFI_UNSUPPORTED       | The caller failed to call `StartLocalSession` before calling `UpdateLocalSession`.                                                                     |
| EFI_DEVICE_ERROR      | Catch-all hardware error.                                                                                                                              |
## `GBL_EFI_FASTBOOT_PROTOCOL.CloseLocalSession()`

### Summary

Terminates the local session and conducts any necessary cleanup.

### Prototype

```c
EFI_STATUS (EFIAPI * GBL_EFI_CLOSE_LOCAL_SESSION)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* Self,
    IN VOID* Ctx,
);
```

### Parameters

*Self*

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure)
instance.

*Ctx*

A pointer to the local session context provided by a call to `StartLocalSession()`.

### Description

Terminates the local fastboot session and conducts necessary cleanup, including
freeing allocated memory, blanking the display, and so forth.
GBL will call this method before any `reboot`, `boot`, or `continue` command from any
fastboot session.

### Status Codes Returned

| Return Code           | Semantics                                                                         |
|:----------------------|:----------------------------------------------------------------------------------|
| EFI_SUCCESS           | The call completed successfully.                                                  |
| EFI_INVALID_PARAMETER | One of *Self* or *Ctx* is `NULL`.                                                 |
| EFI_UNSUPPORTED       | The caller failed to call `StartLocalSession` before calling `CloseLocalSession`. |
| EFI_DEVICE_ERROR      | Catch-all hardware error.                                                         |
