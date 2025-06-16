# GBL EFI AB Slot Protocol

This document describes the GBL AB Slot protocol.
The protocol defines interfaces that can be used by EFI applications
to query and manipulate boot slots.

See this [document](./gbl_ab_boot_flow.md) For details on how GBL uses this
protocol to implement A/B flows.

| **Status**  | Work in progress |
|:------------|-----------------:|
| **Created** |        2024-9-17 |

## `GBL_EFI_AB_SLOT_PROTOCOL`

### Summary

This protocol provides interfaces for platform specific boot slot operations,
such as determining the number of slots and the current target slot,
changing the current target boot slot, marking boot attempts, and more.

This protocol is optional. When implemented, GBL will use it to manage boot
slots and modes. Otherwise, GBL will fully rely on [BCB][bcb] (Bootloader
Control Block) from the `misc` partition for that.

### Glossary

In the context of this protocol, persistent storage refers to storage that is
preserved across warm reboots, but may not survive a cold reboot of the device.

### GUID

```c
// {9a7a7db4-614b-4a08-3df9-006f49b0d80c}
#define GBL_EFI_AB_SLOT_PROTOCOL_GUID                \
  {                                                  \
    0x9a7a7db4, 0x614b, 0x4a08, {                    \
      0x3d, 0xf9, 0x00, 0x6f, 0x49, 0xb0, 0xd8, 0x0c \
    }                                                \
  }
```

### Protocol Version

```c
#define GBL_EFI_AB_SLOT_PROTOCOL_VERSION 0x00010000
```

### Protocol Interface Structure

```c
typedef struct GBL_EFI_AB_SLOT_PROTOCOL {
  // Currently must contain 0x00010000
  UINT32                              Version;
  GBL_EFI_AB_SLOT_LOAD_BOOT_DATA      LoadBootData;
  GBL_EFI_AB_SLOT_GET_SLOT_INFO       GetSlotInfo;
  GBL_EFI_AB_SLOT_GET_CURRENT_SLOT    GetCurrentSlot;
  GBL_EFI_AB_SLOT_GET_ACTIVE_SLOT     GetActiveSlot;
  GBL_EFI_AB_SLOT_SET_ACTIVE_SLOT     SetActiveSlot;
  GBL_EFI_AB_SLOT_SET_SLOT_UNBOOTABLE SetSlotUnbootable;
  GBL_EFI_AB_SLOT_MARK_BOOT_ATTEMPT   MarkBootAttempt;
  GBL_EFI_AB_SLOT_REINITIALIZE        Reinitialize;
  GBL_EFI_AB_SLOT_GET_BOOT_MODE       GetBootMode;
  GBL_EFI_AB_SLOT_SET_BOOT_MODE       SetBootMode;
  GBL_EFI_AB_SLOT_FLUSH               Flush;
} GBL_EFI_AB_SLOT_PROTOCOL;
```

### Parameters

**Version**

The revision to which the `GBL_EFI_AB_SLOT_PROTOCOL` adheres.
All future version must be backwards compatible.
If a future version is not backwards compatible, a different GUID must be used.

**LoadBootData**

Loads slot metadata from persistent storage. Other slot operations may call
this method internally.
See [`GBL_EFI_AB_SLOT_PROTOCOL.LoadBootData()`](#gbl_efi_ab_slot_protocolloadbootdata).

**GetSlotInfo**

Returns information about a slot by index.
See [`GBL_EFI_AB_SLOT_PROTOCOL.GetSlotInfo()`](#gbl_efi_ab_slot_protocolgetslotinfo).

**GetCurrentSlot**

Returns the slot information of the currently booted bootloader.
See [`GBL_EFI_AB_SLOT_PROTOCOL.GetCurrentSlot()`](#gbl_efi_ab_slot_protocolgetcurrentslot).

**GetNextSlot**

Returns the slot information of the next slot decision.
See [`GBL_EFI_AB_SLOT_PROTOCOL.GetNextSlot()`](#gbl_efi_ab_slot_protocolgetnextslot).

**SetActiveSlot**

Marks the specified slot as the active boot target.
See [`GBL_EFI_AB_SLOT_PROTOCOL.SetActiveSlot()`](#gbl_efi_ab_slot_protocolsetactiveslot).

**SetSlotUnbootable**

Marks the specified slot as unbootable.
See [`GBL_EFI_AB_SLOT_PROTOCOL.SetSlotUnbootable()`](#gbl_efi_ab_slot_protocolsetslotunbootable).

**Reinitialize**

Resets slot metadata to a default, initial state.
See [`GBL_EFI_AB_SLOT_PROTOCOL.Reinitialize()`](#gbl_efi_ab_slot_protocolreinitialize).

**GetBootMode**

Gets the boot mode.
See [`GBL_EFI_AB_SLOT_PROTOCOL.GetBootMode()`][get_boot_mode].

**SetBootMode**

Sets the boot mode.
See [`GBL_EFI_AB_SLOT_PROTOCOL.SetBootMode()`][set_boot_mode].

**Flush**

Synchronizes slot metadata with persistent storage. May re-establish data
structure invariants, e.g. recalculate checksums.
See [`GBL_EFI_AB_SLOT_PROTOCOL.Flush()`](#gbl_efi_ab_slot_protocolflush).

## `GBL_EFI_AB_SLOT_PROTOCOL.LoadBootData()`

### Summary

Loads metadata about system boot slots.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_AB_SLOT_LOAD_BOOT_DATA)(
    IN GBL_EFI_AB_SLOT_PROTOCOL*     This,
    OUT GBL_EFI_SLOT_METADATA_BLOCK* Metadata,
);
```

### Related Definitions

```c
typedef enum _GBL_EFI_SLOT_MERGE_STATUS {
  GBL_EFI_SLOT_MERGE_STATUS_NONE = 0,
  GBL_EFI_SLOT_MERGE_STATUS_UNKNOWN,
  GBL_EFI_SLOT_MERGE_STATUS_SNAPSHOTTED,
  GBL_EFI_SLOT_MERGE_STATUS_MERGING,
  GBL_EFI_SLOT_MERGE_STATUS_CANCELLED,
} GBL_EFI_SLOT_MERGE_STATUS;

typedef struct _GBL_EFI_SLOT_METADATA_BLOCK {
  // Value of 1 if persistent metadata tracks slot unbootable reasons.
  UINT8 UnbootableMetadata;
  UINT8 MaxRetries;
  UINT8 SlotCount;
  // See the definition of GBL_EFI_SLOT_MERGE_STATUS.
  UINT8  MergeStatus;
} GBL_EFI_SLOT_METADATA_BLOCK;
```

### Parameters

*This*

A pointer to the [`GBL_EFI_AB_SLOT_PROTOCOL`](#protcol-interface-structure)
instance.

*Metadata*

On return contains device-specific immutable information about the AB slot
implementation. See [`Related Definitions`](#related-definitions) for the layout
of the metadata structure and its fields.

### Description

In addition to information about individual slots, EFI applications need
overarching metadata about AB boot slot implementations.
In particular, implementations might not store persistent metadata detailing why
specific slots are not bootable (i.e. unbootable metadata). Developers may want
to know whether a device supports unbootable metadata to ease in debugging.

Certain operations may be prohibited due to the device's A/B merge status.
For more information about the *MergeStatus* field and Android Virtual A/B, see
the documentation
[here](https://source.android.com/docs/core/ota/virtual_ab/implement).

### Status Codes Returned

| Return Code             | Semantics                                                                                                     |
|:------------------------|:--------------------------------------------------------------------------------------------------------------|
| `EFI_SUCCESS`           | Slot metadata was successfully read from persistent storage.                                                  |
| `EFI_INVALID_PARAMETER` | One of *This* or *Metadata* is `NULL` or improperly aligned.                                                  |
| `EFI_DEVICE_ERROR`      | There was an error while performing the read operation.                                                       |
| `EFI_VOLUME_CORRUPTED`  | The metadata loaded is invalid or corrupt. The caller should call `Reinitialize` before taking other actions. |

## `GBL_EFI_AB_SLOT_PROTOCOL.GetSlotInfo()`

### Summary

Queries info about a boot slot by index.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_AB_SLOT_GET_SLOT_INFO)(
    IN GBL_EFI_AB_SLOT_PROTOCOL* This,
    IN UINT8                     Idx,
    OUT GBL_EFI_SLOT_INFO*       Info,
)
```

### Related Definitions

```c
typedef enum _GBL_EFI_SLOT_UNBOOTABLE_REASON {
  UNKNOWN_REASON = 0,
  NO_MORE_TRIES,
  SYSTEM_UPDATE,
  USER_REQUESTED,
  VERIFICATION_FAILURE,
} GBL_EFI_SLOT_UNBOOTABLE_REASON;

typedef struct _GBL_EFI_SLOT_INFO {
    // One UTF-8 encoded single character
    UINT32 Suffix;
    // Any value other than those explicitly enumerated in
    // GBL_EFI_SLOT_UNBOOTABLE_REASON
    // will be interpreted as UNKNOWN_REASON.
    UINT32 UnbootableReason;
    UINT8  Priority;
    UINT8  Tries;
    // Value of 1 if slot has successfully booted
    UINT8  Successful;
} GBL_EFI_SLOT_INFO;
```

### Parameters

*This*

A pointer to the [`GBL_EFI_AB_SLOT_PROTOCOL`](#protcol-interface-structure)
instance.

*Idx*

The index of the slot to query.

*Info*

On exit contains the metadata for the specified slot.
See [Related Definitions](#related-definitions-1)
for the layout and fields of the metadata structure.

### Description

Developers and EFI applications may wish to query metadata of arbitrary boot
slots as part of debugging or logging.

### Status Codes Returned

| Return Code             | Semantics                                                                                                     |
|:------------------------|:--------------------------------------------------------------------------------------------------------------|
| `EFI_SUCCESS`           | The call completed successfully.                                                                              |
| `EFI_INVALID_PARAMETER` | One of *This* or *Info* is `NULL` or improperly aligned, or the value of *Idx* invalid.                       |
| `EFI_DEVICE_ERROR`      | There was an error reading metadata from persistent storage.                                                  |
| `EFI_VOLUME_CORRUPTED`  | The metadata loaded is invalid or corrupt. The caller should call `Reinitialize` before taking other actions. |

## `GBL_EFI_AB_SLOT_PROTOCOL.GetCurrentSlot()`

### Summary

Returns the slot information of the currently booted bootloader.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_AB_SLOT_GET_CURRENT_SLOT)(
    IN GBL_EFI_AB_SLOT_PROTOCOL* This,
    OUT GBL_EFI_SLOT_INFO*       Info,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_AB_SLOT_PROTOCOL`](#protcol-interface-structure)
instance.

*Info*

On exit contains the metadata for the current slot.
See the definition for [`GBL_EFI_SLOT_INFO`](#related-definitions-1)
for the structure definition.

### Description

Returns the slot of the currently booted bootloader. If bootloader is not
slotted, i.e. the device only has a single slot bootloader instead of A/B, the
function returns `EFI_UNSUPPORTED`.

This is identical to knowing the index of the current slot and calling
`GetSlotInfo()` with that index.

### Status Codes Returned

| Return Code             | Semantics                          |
|:------------------------|:---------------------------------- |
| `EFI_SUCCESS`           | The call completed successfully.   |
| `EFI_UNSUPPORTED`       | Bootloader is not slotted.         |
| `EFI_INVALID_PARAMETER` | One of *This* or *Info* is `NULL`. |

## `GBL_EFI_AB_SLOT_PROTOCOL.GetNextSlot()`

### Summary

Returns the slot information of the next slot decision.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_AB_SLOT_GET_NEXT_SLOT)(
    IN GBL_EFI_AB_SLOT_PROTOCOL* This,
    IN BOOL                      MarkBootAttempt,
    OUT GBL_EFI_SLOT_INFO*       Info,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_AB_SLOT_PROTOCOL`](#protcol-interface-structure)
instance.

*MarkBootAttempt*

The parameter is set to true if caller attempts to load, verify and boot the
returned slot. If the caller only wants to query the next slot decision and
does not intend to cause any state change, it is set to false.

*Info*

On exit contains the metadata for the next slot.
See the definition for [`GBL_EFI_SLOT_INFO`](#related-definitions-1)
for the structure definition.

### Description

The function returns the highest priority bootable slot according to current
slot state.

If parameter `MarkBootAttempt` is true, implementation should updates internal
metadata for the slot such as decrementing retry counters if slot has not been
successful.

If there are no bootable slots, the function **MUST** returns `EFI_NOT_FOUND`.

### Status Codes Returned

| Return Code             | Semantics                                                                                                     |
|:------------------------|:--------------------------------------------------------------------------------------------------------------|
| `EFI_SUCCESS`           | The call completed successfully.                                                                              |
| `EFI_NOT_FOUND`         | There are no bootable slots for the next decision.                                                            |
| `EFI_INVALID_PARAMETER` | One of *This* or *Info* is `NULL` or improperly aligned.                                                      |
| `EFI_DEVICE_ERROR`      | There was an error reading metadata from persistent storage.                                                  |
| `EFI_VOLUME_CORRUPTED`  | The metadata loaded is invalid or corrupt. The caller should call `Reinitialize` before taking other actions. |

## `GBL_EFI_AB_SLOT_PROTOCOL.SetActiveSlot()`

### Summary

Sets the active slot by index. Makes it the highest priority bootable slot.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_AB_SLOT_SET_ACTIVE_SLOT)(
    IN GBL_EFI_AB_SLOT_PROTOCOL* This,
    IN UINT8                     Idx,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_AB_SLOT_PROTOCOL`](#protcol-interface-structure)
instance.

*Idx*

The index of the new active slot.

### Description

Explicitly sets the target boot slot to the one defined by `Idx`.
This clears any unbootable reason metadata the slot may have, resets its tries
remaining to a device specific default, resets its priority to a device specific
default, sets the priority of all other slots to be lower than that of the
target, and clears the slot's *Successful* flag.
All these changes **MUST** be visible in subsequent calls to `GetSlotInfo()`.
Depending on device policy, e.g. lock state, changing the target boot slot
explicitly may be prohibited.

### Status Codes Returned

| Return Code             | Semantics                                                                                                     |
|:------------------------|:--------------------------------------------------------------------------------------------------------------|
| `EFI_SUCCESS`           | The call completed successfully.                                                                              |
| `EFI_INVALID_PARAMETER` | One of *This* or *Info* is `NULL` or improperly aligned, or the value of *Idx* was invalid.                   |
| `EFI_DEVICE_ERROR`      | There was an error reading metadata from persistent storage.                                                  |
| `EFI_VOLUME_CORRUPTED`  | The metadata loaded is invalid or corrupt. The caller should call `Reinitialize` before taking other actions. |
| `EFI_ACCESS_DENIED`     | Device policy prohibited the boot slot target change.                                                         |

## `GBL_EFI_AB_SLOT_PROTOCOL.SetSlotUnbootable()`

### Summary

Marks a slot as unbootable for the provided reason.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_AB_SLOT_SET_SLOT_UNBOOTABLE)(
    IN GBL_EFI_AB_SLOT_PROTOCOL* This,
    IN UINT8                     Idx,
    IN UINT32                    UnbootableReason,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_AB_SLOT_PROTOCOL`](#protcol-interface-structure)
instance.

*Idx*

The index of the slot to mark unbootable.

*UnbootableReason*

The reason the slot is being marked unable to be booted.
See the definition for [`GBL_EFI_SLOT_UNBOOTABLE_REASON`](#related-definitions-1)
for valid values of *UnbootableReason*.

**Note:** Unbootable reason codes are a best-effort debug and RMA helper.
The device's persistent metadata structures may not track unbootable reasons,
and other software that interacts with boot slots may not set unbootable reason
codes accurately.

### Description

Marks a slot as not a valid boot target.
The slot's *Priority*, *TriesRemaining*, and *Successful* fields are all set to
`0`.
Subsequent calls to `GetSlotInfo()` **MUST** reflect these changes to slot info.
If the slot was the current slot, the current boot target will have changed.
This change **MUST** be reflected in subsequent calls to `GetCurrentSlot()`.

If the protocol driver supports tracking slot unbootable reasons, then
subsequent calls to `GetSlotInfo()` **MUST** have the same *UnbootableReason* in
the info structure.

### Status Codes Returned

| Return Code             | Semantics                                                                                                             |
|:------------------------|:----------------------------------------------------------------------------------------------------------------------|
| `EFI_SUCCESS`           | The call completed successfully.                                                                                      |
| `EFI_INVALID_PARAMETER` | *This* is `NULL` or improperly aligned, the value of *Idx* is invalid, or the value of *UnbootableReason* is invalid. |
| `EFI_DEVICE_ERROR`      | There was an error reading metadata from persistent storage.                                                          |
| `EFI_VOLUME_CORRUPTED`  | The metadata loaded is invalid or corrupt. The caller should call `Reinitialize` before taking other actions.         |

## `GBL_EFI_AB_SLOT_PROTOCOL.MarkBootAttempt()`

### Summary

Marks a boot attempt on the current slot.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_AB_SLOT_MARK_BOOT_ATTEMPT)(
    IN GBL_EFI_AB_SLOT_PROTOCOL* This,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_AB_SLOT_PROTOCOL`](#protcol-interface-structure)
instance.

### Description

Updates internal metadata for the current boot target slot.
If the current slot has registered a successful boot, its tries remaining field
is left unchanged.
If there are no slots with non-zero *Successful* or *Tries* fields, the call to
`MarkBootAttempt()` **MUST** return `EFI_ACCESS_DENIED`. The bootloader then
must decide on the next action to take.

Subsequent calls to `GetSlotInfo()` and `GetCurrentSlot()` **MUST** reflect
the decremented tries.

### Status Codes Returned

| Return Code             | Semantics                                                                                                     |
|:------------------------|:--------------------------------------------------------------------------------------------------------------|
| `EFI_SUCCESS`           | The call completed successfully.                                                                              |
| `EFI_INVALID_PARAMETER` | *This* is `NULL` or improperly aligned.                                                                       |
| `EFI_DEVICE_ERROR`      | There was an error reading metadata from persistent storage.                                                  |
| `EFI_VOLUME_CORRUPTED`  | The metadata loaded is invalid or corrupt. The caller should call `Reinitialize` before taking other actions. |
| `EFI_ACCESS_DENIED`     | The current slot has no more tries remaining.                                                                 |

## `GBL_EFI_AB_SLOT_PROTOCOL.Reinitialize()`

### Summary

Reinitializes all boot slot metadata to a known initial state.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_AB_SLOT_REINITIALIZE)(
    IN GBL_EFI_AB_SLOT_PROTOCOL* This,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_AB_SLOT_PROTOCOL`](#protcol-interface-structure)
instance.

### Description

In particular, all slots should have the following fields cleared and set to
device-specific defaults:

* *Priority*
* *Tries*

and have the following fields set to `0`:

* *UnbootableReason*
* *Successful*

This may change the next target boot slot.

### Status Codes Returned

| Return Code             | Semantics                                              |
|:------------------------|:-------------------------------------------------------|
| `EFI_SUCCESS`           | The call completed successfully.                       |
| `EFI_INVALID_PARAMETER` | *This* is `NULL` or improperly aligned.                |
| `EFI_ACCESS_DENIED`     | Device policy prohibited resetting boot slot metadata. |

## `GBL_EFI_AB_SLOT_PROTOCOL.GetBootMode()`

### Summary

Gets the current boot mode.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_AB_SLOT_GET_BOOT_MODE)(
    IN GBL_EFI_AB_SLOT_PROTOCOL* This,
    OUT UINT32*                  Mode,
);
```

### Related Definitions {#gbl_efi_ab_slot_boot_mode_definitions}

```c
typedef enum _GBL_EFI_AB_SLOT_BOOT_MODE {
    NORMAL = 0,
    RECOVERY,
    FASTBOOTD,
    BOOTLOADER,
} GBL_EFI_BOOT_MODE;
```

### Parameters

*This*

A pointer to the [`GBL_EFI_AB_SLOT_PROTOCOL`](#protcol-interface-structure)
instance.

*Mode*

On exit, the boot mode code. See
[related definitions](#gbl_efi_ab_slot_boot_mode_definitions) for the list of
valid codes.

### Description

The boot mode is an Android mechanism for communicating between the running
system and the bootloader to determine a boot target. GBL calls this method
strictly once per boot to obtain current boot mode and handle it following:

| Mode         | GBL Semantics                                     | Bootconfig `androidboot.mode` |
|:-------------|:--------------------------------------------------|:------------------------------|
| `NORMAL`     | Boots into Android userspace.                     | `normal`                      |
| `RECOVERY`   | Boots into Android recovery.                      | `recovery`                    |
| `FASTBOOTD`  | Boots into Android recovery to start `fastbootd`. | `recovery`                    |
| `BOOTLOADER` | Starts GBL fastboot.                              | None                          |

Related configuration:

1. The `androidboot.mode` bootconfig property is used to communicate the current
   mode to Android userspace. It's provided by GBL based on the values from the
   table above but can be overridden by the firmware using
   [GBL_EFI_OS_CONFIGURATION_PROTOCOL.FixupBootConfig][fixup_bootconfig]
   with the `:=` bootconfig operator.

2. `androidboot.reason` is an optional bootconfig property
   [used][bootreason_doc] to pass additional context to help userspace determine
   the reason device was booted or rebooted. It is not provided by GBL, but can
   be set by the firmware using
   [GBL_EFI_OS_CONFIGURATION_PROTOCOL.FixupBootConfig][fixup_bootconfig].

This function may be called multiple times during a single GBL boot session. The
implementation must guarantee that it returns the same value each time, unless
the boot mode has been explicitly changed by GBL via
[`SetBootMode`](set_boot_mode).

**Note:** The boot mode should ONLY be determined by checking persistent
storage. In particular, if a device supports
[`GBL_EFI_FASTBOOT_PROTOCOL`](./gbl_efi_fastboot_protocol.md), the return value
of `GBL_EFI_FASTBOOT_PROTOCOL.ShouldStopInFastboot()` — which checks for
user-activated physical hardware triggers (e.g. holding down a button) — should
NOT affect whether the boot mode returned by [`GetBootMode()`][get_boot_mode]
is `BOOTLOADER`.

### Status Codes Returned

| Return Code             | Semantics                                                                                                     |
|:------------------------|:--------------------------------------------------------------------------------------------------------------|
| `EFI_SUCCESS`           | The call completed successfully.                                                                              |
| `EFI_INVALID_PARAMETER` | One of *This*, *Mode*, is `NULL`, improperly aligned, or has unexpected value.                                |

## `GBL_EFI_AB_SLOT_PROTOCOL.SetBootMode()`

### Summary

Sets the current boot mode.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_AB_SLOT_SET_BOOT_MODE)(
    IN GBL_EFI_AB_SLOT_PROTOCOL* This,
    IN UINT32                    Mode,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_AB_SLOT_PROTOCOL`](#protcol-interface-structure)
instance.

*Mode*

The desired boot mode to set. See
[related definitions](#gbl_efi_ab_slot_boot_mode_definitions) for the list of
valid boot modes.

### Description

Sets the Android boot mode. See [`GetBootMode()`][get_boot_mode] for more
information.

The implementation must guarantee that a following[`GetBootMode()`][get_boot_mode] call returns the previously set value, even
after a warm reset, so it must be persisted.

Used by GBL's fastboot implementation to handle `fastboot reboot <mode>` like
commands.

### Status Codes Returned

| Return Code             | Semantics                                                                               |
|:------------------------|:----------------------------------------------------------------------------------------|
| `EFI_SUCCESS`           | The call completed successfully.                                                        |
| `EFI_INVALID_PARAMETER` | One of *This* or *Mode* is `NULL`, improperly aligned, or has unexpected value.         |

## `GBL_EFI_AB_SLOT_PROTOCOL.Flush()`

### Summary

Writes any slot metadata modifications to persistent storage.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI * GBL_EFI_AB_SLOT_FLUSH)(
    IN GBL_EFI_AB_SLOT_PROTOCOL* This,
);
```

### Parameters

*This*

A pointer to the [`GBL_EFI_AB_SLOT_PROTOCOL`](#protcol-interface-structure)
instance.

### Description

Protocol driver implementations may store modifications to boot slot metadata in
memory before committing changes to storage in a single write operation.
Protocol consumers need a mechanism to instruct the driver that they are
finished operating on boot slots and that changes should be committed.
The implementation should conduct any necessary ancillary tasks, e.g.
recalculating checksums, before writing to storage.
This is an optimization for performance and flash lifetime; implementations are
free to write all modifications to storage as they occur and to define `Flash()`
as a no-op.

### Status Codes Returned

| Return Code        | Semantics                                                 |
|:-------------------|:----------------------------------------------------------|
| `EFI_SUCCESS`      | The call completed successfully.                          |
| `EFI_DEVICE_ERROR` | The device reported a write error during synchronization. |

[set_boot_mode]: #gbl_efi_ab_slot_protocolsetbootmode
[get_boot_mode]: #gbl_efi_ab_slot_protocolgetbootmode
[bootreason_doc]: https://source.android.com/docs/core/architecture/bootloader/boot-reason
[fixup_bootconfig]: ./gbl_os_configuration_protocol.md#FixupBootConfig
[bcb]: https://android.googlesource.com/platform/bootable/recovery/+/main/bootloader_message/include/bootloader_message/bootloader_message.h
