# GBL EFI Debug Protocol

|             |            |
| :---------- | :--------- |
| **Status**  | Pre-frozen |
| **Created** | 2025-09-11 |

## GBL_EFI_DEBUG_PROTOCOL

### Summary

The GBL Debug Protocol is an optional protocol that provides callbacks to the
firmware.

### GUID

```c
// {98ca3da1-c1ac-4402-9c16-7558d3ed5705}
#define GBL_EFI_DEBUG_PROTOCOL_GUID                        \
    {                                                      \
        0x98ca3da1, 0xc1ac, 0x4402, {                      \
            0x9c, 0x16, 0x75, 0x58, 0xd3, 0xed, 0x57, 0x05 \
        }                                                  \
    }
```

### Revision Number

```c
#define GBL_EFI_DEBUG_PROTOCOL_REVISION GBL_PROTOCOL_REVISION(0, 256)
```

See
[GBL Custom Protocol Revisions](efi_protocols.md#gbl-custom-protocol-revisions)
for details about protocol revisions.

### Protocol Interface Structure

```c
typedef struct _GBL_EFI_DEBUG_PROTOCOL {
    UINT64                    Revision;
    GBL_EFI_DEBUG_FATAL_ERROR FatalError;
} GBL_EFI_DEBUG_PROTOCOL;
```

### Parameters

#### Revision

The revision to which the `GBL_EFI_DEBUG_PROTOCOL` adheres. All future revisions
must be backwards compatible. If a future version is not backwards compatible, a
different GUID must be used.

#### FatalError

Alerts the firmware that a fatal error has occured.

## GBL_EFI_DEBUG_PROTOCOL.FatalError()

### Summary

Alerts the firmware that a fatal error has occurred and passes the frame pointer
so that the firmware can store or log backtrace information.

### Prototype

```c
typedef EFI_STATUS (EFIAPI *GBL_EFI_DEBUG_FATAL_ERROR)(
    IN GBL_EFI_DEBUG_PROTOCOL  *Self,
    IN CONST VOID              *FramePtr,
    IN GBL_EFI_DEBUG_ERROR_TAG Tag,
    );
```

### Parameters

#### Self

A pointer to the `GBL_EFI_DEBUG_PROTOCOL` instance.

#### FramePtr [in]

The frame pointer of the calling context. GBL's toolchain requires all builds to
include frame pointers and unwind tables in their ABI. This means that all stack
frames are valid at least up to the most recent ABI boundary. The most recent
ABI boundary is usually the GBL entry point, but it can also be UEFI code to
which GBL passed a callback. Frames further back may be valid, but this depends
on whether the firmware was compiled with frame pointers enabled.

On aarch64, the frame pointer is x29. On x86_64, the frame pointer is rbp. On
RISC-V, the frame pointer is x8.

**Note:** for aarch64 and RISC-V, the register used to store the frame pointer
is an ABI convention, not a hard and fast rule. Compilers may use other
registers to store the frame pointer.

#### Tag [in]

A tag used to describe the type of error being logged. See
[`Related Definitions`](#related-definitions) for expected tags and their
semantics.

### Related Definitions

```c
enum {
    // Error was generated automatically due to an assertion failure
    GBL_EFI_DEBUG_ERROR_TAG_ASSERTION_ERROR,
    // General partition related error
    GBL_EFI_DEBUG_ERROR_TAG_PARTITION,
    // Failed to load required image
    GBL_EFI_DEBUG_ERROR_TAG_LOAD_IMAGE,
    // General boot failure
    GBL_EFI_DEBUG_ERROR_TAG_BOOT_ERROR
};
typedef uint64_t GBL_EFI_DEBUG_ERROR_TAG;
```

### Description

`FatalError()` is called by GBL to alert the firmware that a fatal error has
occurred and that it may be helpful to display or save debugging information for
postmortem analysis. The current frame pointer is passed in case the firmware
wishes to conduct a stack trace, and a best-effort tag is passed to provide
additional context about the cause of the error.

Note: when invoked automatically from the panic handler, e.g. when accessing an
array at an invalid index, the tag variant will be
GBL_EFI_DEBUG_ERROR_TAG_ASSERTION_ERROR. Other tag values are provided on a best
effort basis. Additional tag variants may be added as part of a non-breaking
update.

Note: only fatal errors that occur within GBL will automatically invoke
`FatalError()` as part of the panic handler. Any errors that occur within
protocol drivers or the UEFI runtime are not visible to GBL and will not trigger
automatic calls to `FatalError()`.

### Status Codes Returned

| Return Code             | Semantics                        |
| :---------------------- | :------------------------------- |
| `EFI_SUCCESS`           | The call completed successfully. |
| `EFI_INVALID_PARAMETER` | _Self_ is `NULL`.                |
