# GBL EFI Boot Control Protocol

The protocol defines interfaces that can be used by EFI applications to query
and manipulate boot targets.

See this [document](./gbl_ab_boot_flow.md) for details on how GBL uses this
protocol to implement A/B boot flows.

| **Status**  | Work in progress |
| :---------- | ---------------: |
| **Created** |        2024-9-17 |

## GBL_EFI_BOOT_CONTROL_PROTOCOL

### Summary

This protocol provides interfaces for platform specific boot operations, such as
determining the number of slots, determining the current target slot, and
changing the target boot slot.

### GUID

```c
// {d382db1b-9ac2-11f0-84c7-047bcba96019}
#define GBL_EFI_BOOT_CONTROL_PROTOCOL_GUID           \
  {                                                  \
    0xd382db1b, 0x9ac2, 0x11f0, {                    \
      0x84, 0xc7, 0x04, 0x7b, 0xcb, 0xa9, 0x60, 0x19 \
    }                                                \
  }
```

### Protocol Revision

```c
#define GBL_EFI_BOOT_CONTROL_PROTOCOL_REVISION GBL_PROTOCOL_REVISION(0, 2)
```

See
[GBL Custom Protocol Revisions](efi_protocols.md#gbl-custom-protocol-revisions)
for details about protocol revisions.

### Protocol Interface Structure

```c
typedef struct GBL_EFI_BOOT_CONTROL_PROTOCOL {
  UINT64                                      Revision;
  GBL_EFI_BOOT_CONTROL_GET_SLOT_COUNT         GetSlotCount;
  GBL_EFI_BOOT_CONTROL_GET_SLOT_INFO          GetSlotInfo;
  GBL_EFI_BOOT_CONTROL_GET_CURRENT_SLOT       GetCurrentSlot;
  GBL_EFI_BOOT_CONTROL_SET_ACTIVE_SLOT        SetActiveSlot;
  GBL_EFI_BOOT_CONTROL_GET_ONE_SHOT_BOOT_MODE GetOneShotBootMode;
  GBL_EFI_BOOT_CONTROL_HANDLE_LOADED_OS       HandleLoadedOs;
} GBL_EFI_BOOT_CONTROL_PROTOCOL;
```

### Parameters

**Revision**

The revision to which the `GBL_EFI_BOOT_CONTROL_PROTOCOL` adheres. All future
version must be backwards compatible. If a future version is not backwards
compatible, a different GUID must be used.

**GetSlotCount**

Returns the number of boot slots. See
[`GBL_EFI_BOOT_CONTROL_PROTOCOL.GetSlotCount()`](#gbl_efi_boot_control_protocol_getslotcount).

**GetSlotInfo**

Returns information about a slot by index. See
[`GBL_EFI_BOOT_CONTROL_PROTOCOL.GetSlotInfo()`](#gbl_efi_boot_control_protocol_getslotinfo).

**GetCurrentSlot**

Returns the information of the currently booted slot. See
[`GBL_EFI_BOOT_CONTROL_PROTOCOL.GetCurrentSlot()`](#gbl_efi_boot_control_protocol_getcurrentslot).

**SetActiveSlot**

Marks the specified slot as the active boot target. See
[`GBL_EFI_BOOT_CONTROL_PROTOCOL.SetActiveSlot()`](#gbl_efi_boot_control_protocol_setactiveslot).

**GetOneShotBootMode**

Returns the hardware triggered one-shot boot mode. See
[`GBL_EFI_BOOT_CONTROL_PROTOCOL.GetOneShotBootMode()`](#gbl_efi_boot_control_protocol_getoneshotbootmode).

**HandleLoadedOs**

Handles loaded OS images and provides OS entry point. See
[`GBL_EFI_BOOT_CONTROL_PROTOCOL.HandleLoadedOs()`](#gbl_efi_boot_control_protocol_handleloadedos).

## GBL_EFI_BOOT_CONTROL_PROTOCOL.GetSlotCount()

### Summary

Returns the number of boot slots.

### Prototype

```c
typedef EFI_STATUS (EFIAPI *GBL_EFI_BOOT_CONTROL_GET_SLOT_COUNT)(
    IN GBL_EFI_BOOT_CONTROL_PROTOCOL  *Self,
    OUT UINT8                         *SlotCount
);
```

### Parameters

_Self_

A pointer to the
[`GBL_EFI_BOOT_CONTROL_PROTOCOL`](#protocol-interface-structure) instance.

_SlotCount_

On return contains the number of boot slots.

### Description

Returns the number of boot slots.

This method could be called multiple times during a boot or fastboot session.
Subsequent calls to this method should always return the same value.

### Status Codes Returned

| Return Code             | Semantics                                                     |
| :---------------------- | :------------------------------------------------------------ |
| `EFI_SUCCESS`           | Slot metadata was successfully read from persistent storage.  |
| `EFI_INVALID_PARAMETER` | One of _Self_ or _SlotCount_ is `NULL` or improperly aligned. |

## GBL_EFI_BOOT_CONTROL_PROTOCOL.GetSlotInfo()

### Summary

Queries info about a boot slot by index.

### Prototype

```c
typedef EFI_STATUS (EFIAPI *GBL_EFI_BOOT_CONTROL_GET_SLOT_INFO)(
    IN GBL_EFI_BOOT_CONTROL_PROTOCOL  *Self,
    IN UINT8                          Idx,
    OUT GBL_EFI_SLOT_INFO             *Info
);
```

### Related Definitions

```c
enum {
  GBL_EFI_SLOT_UNBOOTABLE_REASON_UNKNOWN_REASON = 0,
  GBL_EFI_SLOT_UNBOOTABLE_REASON_NO_MORE_TRIES,
  GBL_EFI_SLOT_UNBOOTABLE_REASON_SYSTEM_UPDATE,
  GBL_EFI_SLOT_UNBOOTABLE_REASON_USER_REQUESTED,
  GBL_EFI_SLOT_UNBOOTABLE_REASON_VERIFICATION_FAILURE,
};

typedef uint8_t GBL_EFI_SLOT_UNBOOTABLE_REASON;

typedef struct _GBL_EFI_SLOT_INFO {
    // One UTF-8 encoded single character
    UINT32 Suffix;
    // Any value other than those explicitly enumerated in
    // GBL_EFI_SLOT_UNBOOTABLE_REASON
    // will be interpreted as UNKNOWN_REASON.
    UINT8 UnbootableReason;
    UINT8 Priority;
    // Number of remaining tries to attempt to boot the slot
    UINT8 RemainingTries;
    // Value of 1 if slot has successfully booted
    UINT8 Successful;
} GBL_EFI_SLOT_INFO;
```

### Parameters

_Self_

A pointer to the
[`GBL_EFI_BOOT_CONTROL_PROTOCOL`](#protocol-interface-structure) instance.

_Idx_

The index of the slot to query.

_Info_

On exit contains the metadata for the specified slot. See
[Related Definitions](#related-definitions-1) for the layout and fields of the
metadata structure.

### Description

Developers and EFI applications may wish to query metadata of arbitrary boot
slots as part of debugging or logging.

A slot that is not successful and has no tries left is considered unbootable.

### Status Codes Returned

| Return Code             | Semantics                                                                               |
| :---------------------- | :-------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`           | The call completed successfully.                                                        |
| `EFI_INVALID_PARAMETER` | One of _Self_ or _Info_ is `NULL` or improperly aligned, or the value of _Idx_ invalid. |
| `EFI_DEVICE_ERROR`      | There was an error reading metadata from persistent storage.                            |

## GBL_EFI_BOOT_CONTROL_PROTOCOL.GetCurrentSlot()

### Summary

Returns the information of the currently booted slot.

### Prototype

```c
typedef EFI_STATUS (EFIAPI *GBL_EFI_BOOT_CONTROL_GET_CURRENT_SLOT)(
    IN GBL_EFI_BOOT_CONTROL_PROTOCOL  *Self,
    OUT GBL_EFI_SLOT_INFO             *Info
);
```

### Parameters

_Self_

A pointer to the
[`GBL_EFI_BOOT_CONTROL_PROTOCOL`](#protocol-interface-structure) instance.

_Info_

On exit contains the metadata for the current slot. See the definition for
[`GBL_EFI_SLOT_INFO`](#related-definitions-1) for the structure definition.

### Description

Returns the information of the currently booted slot.

This is identical to knowing the index of the current slot and calling
`GetSlotInfo()` with that index.

### Status Codes Returned

| Return Code             | Semantics                          |
| :---------------------- | :--------------------------------- |
| `EFI_SUCCESS`           | The call completed successfully.   |
| `EFI_INVALID_PARAMETER` | One of _Self_ or _Info_ is `NULL`. |

## GBL_EFI_BOOT_CONTROL_PROTOCOL.SetActiveSlot()

### Summary

Sets the active slot by index. Makes it the highest priority bootable slot.

### Prototype

```c
typedef EFI_STATUS (EFIAPI *GBL_EFI_BOOT_CONTROL_SET_ACTIVE_SLOT)(
    IN GBL_EFI_BOOT_CONTROL_PROTOCOL  *Self,
    IN UINT8                          Idx
);
```

### Parameters

_Self_

A pointer to the
[`GBL_EFI_BOOT_CONTROL_PROTOCOL`](#protocol-interface-structure) instance.

_Idx_

The index of the new active slot.

### Description

Explicitly sets the target boot slot to the one defined by `Idx`. This clears
any unbootable reason metadata the slot may have, resets its tries remaining to
a device specific default, resets its priority to a device specific default,
sets the priority of all other slots to be lower than that of the target, and
clears the slot's _Successful_ flag. All these changes **MUST** be visible in
subsequent calls to `GetSlotInfo()`. Depending on device policy, e.g. lock
state, changing the target boot slot explicitly may be prohibited.

### Status Codes Returned

| Return Code             | Semantics                                                                                   |
| :---------------------- | :------------------------------------------------------------------------------------------ |
| `EFI_SUCCESS`           | The call completed successfully.                                                            |
| `EFI_INVALID_PARAMETER` | One of _Self_ or _Info_ is `NULL` or improperly aligned, or the value of _Idx_ was invalid. |
| `EFI_DEVICE_ERROR`      | There was an error reading metadata from persistent storage.                                |
| `EFI_ACCESS_DENIED`     | Device policy prohibited the boot slot target change.                                       |

## GBL_EFI_BOOT_CONTROL_PROTOCOL.GetOneShotBootMode()

### Summary

Gets the hardware triggered one-shot boot mode.

### Prototype

```c
typedef EFI_STATUS (EFIAPI *GBL_EFI_BOOT_CONTROL_GET_ONE_SHOT_BOOT_MODE)(
    IN GBL_EFI_BOOT_CONTROL_PROTOCOL  *Self,
    OUT UINT32                        *Mode
);
```

### Related Definitions

```c
enum {
  GBL_EFI_ONE_SHOT_BOOT_MODE_NONE = 0,
  GBL_EFI_ONE_SHOT_BOOT_MODE_BOOLOADER,
  GBL_EFI_ONE_SHOT_BOOT_MODE_RECOVERY,
};

typedef uint32_t GBL_EFI_ONE_SHOT_BOOT_MODE;
```

### Parameters

_Self_

A pointer to the
[`GBL_EFI_BOOT_CONTROL_PROTOCOL`](#protocol-interface-structure) instance.

_Mode_

On exit contains the overridding boot mode. See the definition of
`GBL_EFI_ONE_SHOT_BOOT_MODE` for the possible value.

### Description

Devices often define custom mechanisms for determining whether to enter
bootloader or recovery mode on boot. For example, press and hold the "volume
down" button while booting the device.

This method checks whether a hardware key combo is triggered, and returns the
triggered one-shot boot mode.

Note: This method should **only** return boot mode caused by hardware button
press. It must not check the boot mode provided by metadata stored on persistent
storage.

### Status Codes Returned

| Return Code             | Semantics                                                |
| :---------------------- | :------------------------------------------------------- |
| `EFI_SUCCESS`           | The call completed successfully.                         |
| `EFI_INVALID_PARAMETER` | One of _Self_ or _Mode_ is `NULL` or improperly aligned. |

## GBL_EFI_BOOT_CONTROL_PROTOCOL.HandleLoadedOs()

### Summary

Handles loaded OS images and provides OS entry point.

### Prototype

```c
typedef EFI_STATUS (EFIAPI *GBL_EFI_BOOT_CONTROL_HANDLE_LOADED_OS)(
    IN GBL_EFI_BOOT_CONTROL_PROTOCOL  *Self,
    IN CONST GBL_EFI_LOADED_OS        *Os,
    OUT OS_ENTRY_POINT                *EntryPoint
);
```

### Parameters

_Self_

A pointer to the
[`GBL_EFI_BOOT_CONTROL_PROTOCOL`](#protocol-interface-structure) instance.

_Os_

A pointer to a `GBL_EFI_LOADED_OS` structure representing the loaded OS images.
The underlying images are guaranteed to remain at the same physical address
across `HandleLoadedOs` and `EntryPoint` calls — they are never relocated by
GBL. However, the `Os` pointer itself is only valid within this call and must
not be retained.

_EntryPoint_

On exit, contains a function pointer to the firmware-specific hardware
preparation and kernel jump logic. It may remain untouched if no custom
implementation is provided, in which case GBL's default handoff logic will be
used. See `OS_ENTRY_POINT` definition below for more details about the expected
input.

Note: The provided function is executed after `ExitBootServices()` is called by
GBL, so provided implementation must not rely on any Boot Services.

Note: The provided function is expected to take over the subsequent boot chain
steps and must never return to GBL. If control returns to GBL, it is treated as
a fatal error.

### Related Definitions

#### GBL_EFI_LOADED_OS

```c
typedef struct _GBL_EFI_LOADED_OS {
  UINTN                 KernelSize;
  EFI_PHYSICAL_ADDRESS  Kernel;
  UINTN                 RamdiskSize;
  EFI_PHYSICAL_ADDRESS  Ramdisk;
  UINTN                 DeviceTreeSize;
  EFI_PHYSICAL_ADDRESS  DeviceTree;
  UINT64                Reserved[8];
} GBL_EFI_LOADED_OS;
```

_KernelSize_

The size of provided `Kernel`.

_Kernel_

Physical memory address of `KernelSize` bytes containing the loaded kernel image
GBL uses for boot.

_RamdiskSize_

The size of provided `Ramdisk`.

_Ramdisk_

Physical memory address of `RamdiskSize` bytes containing the loaded ramdisk GBL
uses for boot.

_DeviceTreeSize_

The size of provided `DeviceTree`.

_DeviceTree_

Physical memory address of `DeviceTreeSize` bytes containing the loaded device
tree GBL uses for boot.

_Reserved_

Reserved for future use.

#### OS_ENTRY_POINT

```c
typedef VOID (*OS_ENTRY_POINT)(
    IN UINTN                        DescriptorSize,
    IN UINT32                       DescriptorVersion,
    IN UINTN                        NumDescriptors,
    IN CONST EFI_MEMORY_DESCRIPTOR  *MemoryMap,
    IN CONST GBL_EFI_LOADED_OS      *Os
);
```

_DescriptorSize_

The size, in bytes, of an `EFI_MEMORY_DESCRIPTOR` structure.

_DescriptorVersion_

The version number associated with the provided `EFI_MEMORY_DESCRIPTOR` items.

_NumDescriptors_

The number of `EFI_MEMORY_DESCRIPTOR` items provided by `MemoryMap`.

_MemoryMap_

A pointer to the array of `EFI_MEMORY_DESCRIPTOR` representing the memory map
GBL provided to `ExitBootServices()` prior to entry point call.

_Os_

A pointer to a `GBL_EFI_LOADED_OS` structure representing the loaded OS images.
The provided physical addresses are meant to be used directly by the kernel
handoff implementation.

### Description

This method allows the firmware to handle OS images after they have been loaded
by GBL. It can be used to inspect the final kernel, ramdisk, and device tree
images before the kernel handoff to finalize internal state or perform
additional verification steps beyond those handled by GBL.

The `EntryPoint` function pointer output argument allows the firmware to
override GBL's handoff implementation with device-specific hardware preparation
and kernel jump logic. See the `EntryPoint` documentation above for details.

This method is optional. Returning either `EFI_SUCCESS` or `EFI_UNSUPPORTED` has
the same effect - GBL continues the boot process.

### Status Codes Returned

| Return Code              | Semantics                                                                                  |
| :----------------------- | :----------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`            | OS images are handled successfully.                                                        |
| `EFI_UNSUPPORTED`        | FW does not need to handle OS images. GBL continues to boot.                               |
| `EFI_INVALID_PARAMETER`  | One of _Self_, _Os_, or _EntryPoint_ is `NULL` or improperly aligned. GBL rejects to boot. |
| `EFI_SECURITY_VIOLATION` | Provided OS images fail to meet the device's security requirements. GBL rejects to boot.   |
| `EFI_DEVICE_ERROR`       | Any other error occurred while handling OS images. GBL rejects to boot.                    |
