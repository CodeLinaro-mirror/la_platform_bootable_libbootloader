# GBL EFI Fastboot Protocol

This document describes the GBL Fastboot protocol. The protocol defines
interfaces that can be used by EFI applications to query and modify
vendor-specific information on a device that may be desired in the context of a
fastboot environment.

|             |                    |
| :---------- | -----------------: |
| **Status**  | _Work in progress_ |
| **Created** |          2024-9-11 |

## `GBL_EFI_FASTBOOT_PROTOCOL`

### Summary

This protocol provides interfaces for platform-specific operations during
Fastboot. This can include support for vendor defined variables or variables
whose query requires cooperation with vendor firmware, OEM commands,

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
#define GBL_EFI_FASTBOOT_PROTOCOL_REVISION GBL_PROTOCOL_REVISION(0, 6)
```

See
[GBL Custom Protocol Revisions](efi_protocols.md#gbl-custom-protocol-revisions)
for details about protocol revisions.

### Protocol Interface Structure

```c
#define GBL_EFI_FASTBOOT_SERIAL_NUMBER_MAX_LEN_UTF8 32

typedef struct _GBL_EFI_FASTBOOT_PROTOCOL {
  UINT64                                        Revision
  CHAR8                                         SerialNumber[GBL_EFI_FASTBOOT_SERIAL_NUMBER_MAX_LEN_UTF8];
  GBL_EFI_FASTBOOT_GET_VAR                      GetVar;
  GBL_EFI_FASTBOOT_GET_VAR_ALL                  GetVarAll;
  GBL_EFI_FASTBOOT_GET_STAGED                   GetStaged;
  GBL_EFI_FASTBOOT_COMMAND_EXEC                 CommandExec;
  GBL_EFI_FASTBOOT_GET_PARTITION_TYPE           GetPartitionType;
} GBL_EFI_FASTBOOT_PROTOCOL;
```

### Parameters

**Revision**

The revision to which the `GBL_EFI_FASTBOOT_PROTOCOL` adheres. All future
revisions must be backwards compatible. If a future version is not backwards
compatible, a different GUID must be used.

**SerialNumber**

The device serial number expressed as a Null-terminated UTF-8 encoded string. If
the device serial number is 32 bytes long, the Null terminator must be excluded.
If the device serial number is longer than 32 bytes, it must be truncated.

**GetVar**

Gets the value for the given fastboot variable. See
[`GBL_EFI_FASTBOOT_PROTOCOL.GetVar()`](#gbl_efi_fastboot_protocolgetvar).

**GetVarAll**

Iterates all combinations of arguments and values for all fastboot variables.
See
[`GBL_EFI_FASTBOOT_PROTOCOL.GetVarAll()`](#gbl_efi_fastboot_protocolgetvarall).

**GetStaged**

Read OEM provided payload for uploading to fastboot host by command
`fastboot get_staged`. See
[`GBL_EFI_FASTBOOT_PROTOCOL.GetStaged()`](#gbl_efi_fastboot_protocolgetstaged).

**CommandExec**

Allows custom overriding of fastboot commands. See
[`GBL_EFI_FASTBOOT_PROTOCOL.CommandExec()`](#gbl_efi_fastboot_protocolcommandexec).

**GetPartitionType**

Gets the type of partition. See
[`GBL_EFI_FASTBOOT_PROTOCOL.GetPartitionType()`](#gbl_efi_fastboot_protocol_getpartitiontype).

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

_Self_

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure)
instance.

_Args_

A pointer to an array of NULL-terminated strings that contains the name of the
variable followed by additional arguments.

_NumArgs_

The number of elements in the _Args_ array.

_Buf_

A pointer to the data buffer to store the value of the variable as a UTF-8
encoded string.

_BufSize_

On entry, the size in bytes of _Buf_. On exit, the size in bytes of the UTF-8
encoded string describing the value, excluding any Null-terminator.

### Description

`GetVar()` queries internal data structures and drivers to determine the value
of the given variable. Variables may have zero or more additional arguments.
These arguments are parsed by the caller and passed to `GetVar()` as an array of
NULL-terminated UTF-8 encoded string.

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
| :---------------------- | :----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`           | The variable was found and its value successfully serialized.                                                                                                            |
| `EFI_INVALID_PARAMETER` | One of _Self_, _Args_, _Buf_, or _BufSize_ is `NULL`                                                                                                                     |
| `EFI_NOT_FOUND`         | The first element of _Args_ does not contain a known variable.                                                                                                           |
| `EFI_UNSUPPORTED`       | The contents of _Args_ do not contain a known variable with valid aruments. Any of the subarguments may be unknown, or too many or too few subarguments may be provided. |
| `EFI_BUFFER_TOO_SMALL`  | _Buf_ is too small to store the serialized variable string. The value of _BufSize_ is modified to contain the minimum necessary buffer size.                             |

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

_Self_

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure)
instance.

_Context_

A pointer to the context data for `GetVarAllCallback`.

_GetVarAllCallback_

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

_Context_

The pointer to the context passed to `GetVarAll()`.

_Args_

A pointer to an array of NULL-terminated strings that contains the name of the
variable followed by additional arguments.

The name and arguments correspond to the `:` separated variable format by the
fastboot protocol, i.e. `fastboot getvar <name>:<arg1>:<arg2>..`. However
firmware may also choose to pass the entire `"<name>:<arg1>:<arg2>.."` string as
a 1-size array if preferred. Caller should expect both cases.

_NumArgs_

The number of elements in the _Args_ array.

_Value_

A NULL-terminated string representing the value.

### Description

`GetVarAll()` iterates all combinations of arguments and values for all fastboot
variables. For each combination, the function invokes the caller provided
callback `GetVarAllCallback()` and passes the context, arguments and value.

### Status Codes Returned

| Return Code             | Semantics                                       |
| :---------------------- | :---------------------------------------------- |
| `EFI_SUCCESS`           | Operation is successful.                        |
| `EFI_INVALID_PARAMETER` | One of _Self_ or _GetVarAllCallback_ is `NULL`. |

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
    OUT UINT8*                    Out,
    IN OUT UINTN*                 OutLen,
    OUT UINTN*                    RemainingSize
);
```

### Parameters

_Self_

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure)
instance.

_Out_

Pointer to the output buffer.

_OutLen_

On input, stores the size of the output buffer `Out`. On output, stores the
actual number of bytes read to `Out`.

_RemainingSize_

On output, stores the number of remaining bytes left to read.

### Description

`GetStaged()` reads OEM defined data for uploading to fastboot host during
command `fastboot get_staged`. The function may be called multiple times to read
out the whole payload in chunks to accommodate callers with limited buffer.
Implementation should internally track read progress and avoid changing the
backing data when caller starts reading. However, outside the session of
`fastboot get_staged`, i.e. when in `RunOemFunction`, implementation can change
or update the backing data.

Caller may pass a 0-length input buffer for peeking the total via
`RemainingSize`. This should be expected by the implementation.

The typical usage is to for vendor to provide an OEM command that sets up the
payload and then retrieve the payload via `fastboot get_staged` from the host.

### Status Codes Returned

| Return Code             | Semantics                                                 |
| :---------------------- | :-------------------------------------------------------- |
| `EFI_SUCCESS`           | The call completed successfully.                          |
| `EFI_INVALID_PARAMETER` | Any of _Out_, _OutLen_, _RemainingSize_ is `NULL`.        |
| `EFI_ACCESS_DENIED`     | The operation is not permitted in the current lock state. |

## `GBL_EFI_FASTBOOT_PROTOCOL.CommandExec()`

### Summary

Allows for command filtering and implementation override.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_COMMAND_EXEC)(
    IN GBL_EFI_FASTBOOT_PROTOCOL* Self,
    IN UINTN                                  NumArgs,
    IN CONST CHAR8* CONST*                    Args,
    IN UINTN                                  DownloadDataLen,
    IN UINT8*                                 DownloadData,
    IN UINTN                                  DownloadDataFullSize,
    OUT GBL_EFI_FASTBOOT_COMMAND_EXEC_RESULT* Implementation,
    IN FASTBOOT_MESSAGE_SENDER                Sender,
    IN VOID*                                  SenderContext,
);
```

### Parameters

_Self_

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure)
instance.

_NumArgs_

The number of elements in the _Args_ array.

_Args_

A pointer to an array of NULL-terminated UTF-8 strings that contains the
fastboot command followed by additional arguments.

_DownloadData_

A pointer to the most recent downloaded data.

_DownloadDataLen_

The size of the download data in `DownloadData`.

`DownloadData` and `DownloadDataLen` provide additional context for commands
such as `fastboot flash`.

_DownloadDataFullSize_

Full size of the download data buffer `DownloadData`. It can be bigger than
`DownloadDataLen` for custom implementation use.

_Implementation_

On exit, set to one of the values:

- GBL_EFI_FASTBOOT_COMMAND_EXEC_RESULT_PROHIBITED - command is not allowed
- GBL_EFI_FASTBOOT_COMMAND_EXEC_RESULT_DEFAULT_IMPL - GBL is required to pass
  this variant as the default value. The callee can leave the parameter
  untouched if 'DEFAULT_IMPL' is the desired behavior and just return
  `EFI_SUCCESS`.
- GBL_EFI_FASTBOOT_COMMAND_EXEC_RESULT_CUSTOM_IMPL - GBL should ignore the
  command since custom implementation was used

_Sender_

A pointer to a function of type `FASTBOOT_MESSAGE_SENDER`. The function is used
by the implementation to send custom fastboot OKAY/FAIL/INFO messages. For input
arguments, it takes the `SenderContext` pointer passed to this function, the
message type, a pointer to a UTF8 string and the string length.

Warning: The `Sender` parameter should only be used for commands that are
`CUSTOM_IMPL`. Using Sender to send messages for commands that are `PROHIBITED`
or `DEFAULT_IMPL` will result in undefined behavior, incorrect output, or will
leave the fastboot protocol in a bad state.

OKAY/FAIL messages should only be sent once. Sending more than one OKAY or FAIL
or sending both may break the fastboot exchange sequence. It is the caller's
responsibility to provide a sender function that verifies that at most one OKAY
OR FAIL message is sent and returns EFI_PROTOCOL_ERROR if the implementation
violates this requirement.

Likewise if implementation returns without sending any OKAY/FAIL message, caller
should send either an OKAY or a FAIL based on the return value of this API.

_SenderContext_

A pointer to the context data for `Sender`.

### Description

`CommandExec()` queries whether a fastboot command is allowed and what
implementation to use. If command is not allowed, firmware can output an
optional NULL-terminated message in `MsgBuf`.

It's up to the caller to decide how to proceed in the case of error, i.e base on
the level of security requirement.

If "default implementation" is requested GBL will handle the command using an
implementation within GBL.

If "custom implementation" is indicated GBL will assume that the callee handled
the command.

Following commands can not be overridden:

<!-- LINT.IfChange -->

- `continue`
- `download`
- `fetch`
- `getvar`
- `reboot`
- `reboot-bootloader`
- `reboot-fastboot`
- `reboot-recovery`
- `set_active`
- `upload`
<!-- LINT.ThenChange(/gbl/libfastboot/src/lib.rs) -->

### Status Codes Returned

| Return Code             | Semantics                                                                                                    |
| :---------------------- | :----------------------------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`           | The call completed successfully.                                                                             |
| `EFI_INVALID_PARAMETER` | `Command` or `Implementation` or `MsgBuf` is NULL. `DownloadDataLen` is non-zero but `DownloadData` is NULL. |
| `EFI_DEVICE_ERROR`      | An internal error occurred.                                                                                  |

### Related Definitions

```c
enum {
  GBL_EFI_FASTBOOT_COMMAND_EXEC_RESULT_PROHIBITED,
  GBL_EFI_FASTBOOT_COMMAND_EXEC_RESULT_DEFAULT_IMPL,
  GBL_EFI_FASTBOOT_COMMAND_EXEC_RESULT_CUSTOM_IMPL,
};
typedef uint32_t GBL_EFI_FASTBOOT_COMMAND_EXEC_RESULT;

enum {
  GBL_EFI_FASTBOOT_MESSAGE_TYPE_OKAY,
  GBL_EFI_FASTBOOT_MESSAGE_TYPE_FAIL,
  GBL_EFI_FASTBOOT_MESSAGE_TYPE_INFO,
};
typedef uint32_t GBL_EFI_FASTBOOT_MESSAGE_TYPE;

typedef
EFI_STATUS (*FASTBOOT_MESSAGE_SENDER) (
    IN VOID*                      Context,
    IN EFI_FASTBOOT_MESSAGE_TYPE  MsgType
    IN CONST CHAR8*               Msg,
    IN UINTN                      Len,
);
```

_GBL_EFI_FASTBOOT_COMMAND_EXEC_RESULT_PROHIBITED_ - indicates that command is
not allowed. GBL is responsible for communicating the prohibition to the user.
This is a convenience common case and GBL will send a generic error message. A
custom error message can be sent if the exec result is
`GBL_EFI_FASTBOOT_COMMAND_EXEC_RESULT_CUSTOM_IMPL`.

_GBL_EFI_FASTBOOT_COMMAND_EXEC_RESULT_DEFAULT_IMPL_ - GBL will use its own
default implementation to handle the command.

_GBL_EFI_FASTBOOT_COMMAND_EXEC_RESULT_CUSTOM_IMPL_ - command is handled by
running custom implementation. Vendor firmware is responsible for guaranteeing
that the implementation has run to completion before returning this value. GBL
will ignore the command assuming it has been handled.

_Context_

The pointer to the context passed to `RunOemFunction()`.

_MsgType_

A `EFI_FASTBOOT_MESSAGE_TYPE` value indicating message type.

_Msg_

A pointer to a UTF8 string. The string does not need to be NULL terminated.

_Len_

The length of `Msg`.

Note: The max allowed length of a message depends on the transport. For example,
for Fastboot over USB, it is the native packet size. Implementation should
consider the transport setup it provides when passing the string. Oversized
message may be truncated by the caller when sent to the host.

## GBL_EFI_FASTBOOT_PROTOCOL.GetPartitionType()

### Summary

Gets the type of partition.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_FASTBOOT_GET_PARTITION_TYPE)(
    IN GBL_EFI_FASTBOOT_PROTOCOL*   Self,
    IN CONST CHAR8*                 PartName,
    OUT CHAR8*                      PartType,
    IN OUT UINTN*                   PartTypeLen
);
```

### Related Definitions

```c
static const UINTN GBL_EFI_FASTBOOT_PARTITION_TYPE_BUF_LEN = 56;
```

### Parameters

_Self_

A pointer to the [`GBL_EFI_FASTBOOT_PROTOCOL`](#protocol-interface-structure)
instance.

_PartName_

The NULL-terminated name of the partition to query.

_PartType_

A buffer to write the result to. This does not need to be NULL-terminated.

_PartTypeLen_

On entry, the size of the _PartType_ buffer. This should be larger or equal to
`GBL_EFI_FASTBOOT_PARTITION_TYPE_BUF_LEN`.

On exit, the size in bytes of the _PartType_ string, excluding any
NULL-terminator.

### Description

This API is for supporting `fastboot format`. The partition type returned here
would be reported to the fastboot client in `getvar partition-type:<partname>`.

If the partition should support formatting with `fastboot format`, then
_PartType_ should contain the filesystem type to format to.

If a partition or all partitions shouldn't support `fastboot format`, then this
API can be left unimplemented or return `EFI_UNSUPPORTED`. In that case GBL
reports the partition type as `raw`.

### Status Codes Returned

| Return Code             | Semantics                                                                                                                              |
| :---------------------- | :------------------------------------------------------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`           | The call completed successfully.                                                                                                       |
| `EFI_INVALID_PARAMETER` | One of _Self_, _PartName_, or _PartType_ is `NULL`.                                                                                    |
| `EFI_UNSUPPORTED`       | _PartName_ is a raw partition that doesn't support `fastboot format`.                                                                  |
| `EFI_BUFFER_TOO_SMALL`  | _PartType_ buffer is too small to store the result. The value of _PartTypeLen_ should be updated to the minimum necessary buffer size. |
