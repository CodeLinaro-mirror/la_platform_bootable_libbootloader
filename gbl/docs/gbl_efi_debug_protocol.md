# GBL EFI Debug Protocol

## GBL_EFI_DEBUG_PROTOCOL

### Summary

The GBL Debug Protocol is an optional protocol that provides callbacks to the firmware.

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
#define GBL_EFI_DEBUG_PROTOCOL_REVISION GBL_PROTOCOL_REVISION(0, 1)
```

See [GBL Custom Protocol Revisions](efi_protocols.md#gbl-custom-protocol-revisions) for details about protocol revisions.

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
must be backwards compatible. If a future version is not backwards compatible,
a different GUID must be used.

#### FatalError

Alerts the firmware that a fatal error has occured.

**Note:** this method will only be called in the GBL panic handler. On return, GBL
will attempt to reset the system. If the reset fails, GBL will hang via loop.

## GBL_EFI_DEBUG_PROTOCOL.FatalError()

### Summary

Alerts the firmware that a fatal error has occurred and passes the frame pointer
so that the firmware can store or log backtrace information.

### Prototype

```c
typedef EFI_STATUS (EFIAPI *GBL_EFI_DEBUG_FATAL_ERROR)(
    IN GBL_EFI_DEBUG_PROTOCOL *Self,
    IN CONST VOID             *FramePtr,
    );
```

### Parameters

#### Self

A pointer to the `GBL_EFI_DEBUG_PROTOCOL` instance.

#### FramePtr [in]

The frame pointer of the calling context. GBL's toolchain requires all builds to
include frame pointers and unwind tables in their ABI. This means that all stack frames
are valid at least up to the most recent ABI boundary. The most recent ABI boundary
is usually the GBL entry point, but it can also be UEFI code to which GBL passed a callback.
Frames further back may be valid, but this depends on whether the firmware was compiled
with frame pointers enabled.

On aarch64, the frame pointer
is x29. On x86_64, the frame pointer is rbp. On RISC-V, the frame pointer is x8.

**Note:** for aarch64 and RISC-V, the register used to store the frame pointer is an
ABI convention, not a hard and fast rule. Compilers may use other registers to store
the frame pointer.

### Status Codes Returned

| Return Code             | Semantics                        |
|:------------------------|:---------------------------------|
| `EFI_SUCCESS`           | The call completed successfully. |
| `EFI_INVALID_PARAMETER` | *Self* is `NULL`.                |
