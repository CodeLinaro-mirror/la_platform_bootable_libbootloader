# GBL EFI Boot Control Protocol

|                |            |
| :------------- | :--------- |
| **Status**     | Stable     |
| **Created**    | 2024-9-17  |
| **Stabilized** | 2026-05-22 |

The protocol defines interfaces that can be used by EFI applications to query
and manipulate boot targets.

See this [document][ab_boot_flow] for details on how GBL uses this protocol to
implement A/B boot flows.

## GBL_EFI_BOOT_CONTROL_PROTOCOL

### Summary

This protocol provides interfaces for platform specific boot operations, such as
determining the number of slots, determining the current target slot, and
changing the target boot slot.

See the [GBL A/B Boot Flow][ab_boot_flow] document for details on how GBL uses
this protocol to implement A/B boot flows.

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

### Revision Number

```c
#define GBL_EFI_BOOT_CONTROL_PROTOCOL_REVISION GBL_PROTOCOL_REVISION(1, 0)
```

See [GBL Custom Protocol Revisions][custom_protocol_revisions] for details about
protocol revisions.

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

#### Revision

The revision to which the `GBL_EFI_BOOT_CONTROL_PROTOCOL` adheres. All future
revisions must be backwards compatible. If a future version is not backwards
compatible, a different GUID must be used.

#### GetSlotCount

Returns the number of boot slots. See [`GetSlotCount()`][get_slot_count] for
more information.

#### GetSlotInfo

Returns information about a slot by index. See [`GetSlotInfo()`][get_slot_info]
for more information.

#### GetCurrentSlot

Returns information about the currently booted slot. See
[`GetCurrentSlot()`][get_current_slot] for more information.

#### SetActiveSlot

Marks the specified slot as the active boot target. See
[`SetActiveSlot()`][set_active_slot] for more information.

#### GetOneShotBootMode

Returns the hardware-triggered one-shot boot mode. See
[`GetOneShotBootMode()`][get_one_shot_boot_mode] for more information.

#### HandleLoadedOs

Handles loaded OS images and allows for overriding the OS entry point logic. See
[`HandleLoadedOs()`][handle_loaded_os] for more information.

## GBL_EFI_BOOT_CONTROL_PROTOCOL.GetSlotCount()

### Summary

Returns the number of boot slots.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_BOOT_CONTROL_GET_SLOT_COUNT)(
  IN GBL_EFI_BOOT_CONTROL_PROTOCOL *Self,
  OUT UINT8                        *SlotCount
  );
```

### Parameters

#### Self

A pointer to the `GBL_EFI_BOOT_CONTROL_PROTOCOL` instance.

#### SlotCount

An output parameter that will contain the number of boot slots.

### Description

Returns the number of boot slots available on the device.

This method may be called multiple times during a boot or Fastboot session.
Subsequent calls to this method must always return the same value.

### Status Codes Returned

| Return Code             | Semantics                                                    |
| :---------------------- | :----------------------------------------------------------- |
| `EFI_SUCCESS`           | Slot metadata was successfully read from persistent storage. |
| `EFI_INVALID_PARAMETER` | `Self` or `SlotCount` is `NULL`.                             |

## GBL_EFI_BOOT_CONTROL_PROTOCOL.GetSlotInfo()

### Summary

Queries information about a boot slot by index.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_BOOT_CONTROL_GET_SLOT_INFO)(
  IN GBL_EFI_BOOT_CONTROL_PROTOCOL *Self,
  IN UINT8                         Idx,
  OUT GBL_EFI_SLOT_INFO            *Info
  );
```

### Related Definitions

#### GBL_EFI_SLOT_UNBOOTABLE_REASON

```c
enum {
  GBL_EFI_SLOT_UNBOOTABLE_REASON_UNKNOWN_REASON = 0,
  GBL_EFI_SLOT_UNBOOTABLE_REASON_NO_MORE_TRIES,
  GBL_EFI_SLOT_UNBOOTABLE_REASON_SYSTEM_UPDATE,
  GBL_EFI_SLOT_UNBOOTABLE_REASON_USER_REQUESTED,
  GBL_EFI_SLOT_UNBOOTABLE_REASON_VERIFICATION_FAILURE,
};

typedef UINT8 GBL_EFI_SLOT_UNBOOTABLE_REASON;
```

#### GBL_EFI_SLOT_INFO

```c
typedef struct _GBL_EFI_SLOT_INFO {
  // One UTF-8 encoded single character
  UINT32                         Suffix;
  GBL_EFI_SLOT_UNBOOTABLE_REASON UnbootableReason;
  UINT8                          Priority;
  UINT8                          RemainingTries;
  UINT8                          Successful;
} GBL_EFI_SLOT_INFO;
```

##### Suffix

A single UTF-8 encoded character representing the slot suffix (e.g., 'a' or
'b').

##### UnbootableReason

A `GBL_EFI_SLOT_UNBOOTABLE_REASON` value indicating why the slot is considered
unbootable. This field is only used when the slot is in an unbootable state
(`RemainingTries` and `Successful` are both zero).

##### Priority

The boot priority of the slot. Higher values indicate higher priority.

##### RemainingTries

The number of remaining attempts to boot this slot before it is marked
unbootable.

##### Successful

Set to 1 if the slot has successfully booted in the past.

### Parameters

#### Self

A pointer to the `GBL_EFI_BOOT_CONTROL_PROTOCOL` instance.

#### Idx

The index of the slot to query.

#### Info

An output parameter that will contain the metadata for the specified slot. See
[GBL_EFI_SLOT_INFO][slot_info] for the layout of the metadata structure.

### Description

This method allows GBL or other EFI applications to query metadata for arbitrary
boot slots. This is useful for debugging, logging, or implementing boot logic.

A slot that is not marked as successful and has zero tries remaining is
considered unbootable.

### Status Codes Returned

| Return Code             | Semantics                                                     |
| :---------------------- | :------------------------------------------------------------ |
| `EFI_SUCCESS`           | The call completed successfully.                              |
| `EFI_INVALID_PARAMETER` | `Self` or `Info` is `NULL`, or the value of `Idx` is invalid. |
| `EFI_DEVICE_ERROR`      | There was an error reading metadata from persistent storage.  |

## GBL_EFI_BOOT_CONTROL_PROTOCOL.GetCurrentSlot()

### Summary

Returns information about the currently booted slot.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_BOOT_CONTROL_GET_CURRENT_SLOT)(
  IN GBL_EFI_BOOT_CONTROL_PROTOCOL *Self,
  OUT GBL_EFI_SLOT_INFO            *Info
  );
```

### Parameters

#### Self

A pointer to the `GBL_EFI_BOOT_CONTROL_PROTOCOL` instance.

#### Info

An output parameter that will contain the metadata for the current slot. See
[GBL_EFI_SLOT_INFO][slot_info] for the structure definition.

### Description

Returns information about the slot that is currently being used for the boot
process.

This is equivalent to determining the index of the current slot and calling
[`GetSlotInfo()`][get_slot_info] with that index.

### Status Codes Returned

| Return Code             | Semantics                        |
| :---------------------- | :------------------------------- |
| `EFI_SUCCESS`           | The call completed successfully. |
| `EFI_INVALID_PARAMETER` | `Self` or `Info` is `NULL`.      |

## GBL_EFI_BOOT_CONTROL_PROTOCOL.SetActiveSlot()

### Summary

Sets the active slot by index, making it the highest priority bootable slot.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_BOOT_CONTROL_SET_ACTIVE_SLOT)(
  IN GBL_EFI_BOOT_CONTROL_PROTOCOL *Self,
  IN UINT8                         Idx
  );
```

### Parameters

#### Self

A pointer to the `GBL_EFI_BOOT_CONTROL_PROTOCOL` instance.

#### Idx

The index of the new active slot.

### Description

Explicitly sets the target boot slot to the one specified by `Idx`. This
operation performs the following actions on the target slot:

1. Clears any unbootable reason metadata.
2. Resets the number of tries remaining to a device-specific default.
3. Resets the priority to a device-specific default.
4. Ensures the priority of all other slots is lower than that of the target
   slot.
5. Clears the slot's `Successful` flag.

All these changes must be visible in subsequent calls to `GetSlotInfo()`.
Depending on device policy (e.g., lock state), explicitly changing the target
boot slot may be prohibited.

### Status Codes Returned

| Return Code             | Semantics                                                  |
| :---------------------- | :--------------------------------------------------------- |
| `EFI_SUCCESS`           | The call completed successfully.                           |
| `EFI_INVALID_PARAMETER` | `Self` is `NULL`, or the value of `Idx` is invalid.        |
| `EFI_DEVICE_ERROR`      | There was an error writing metadata to persistent storage. |
| `EFI_ACCESS_DENIED`     | Device policy prohibited the boot slot change.             |

## GBL_EFI_BOOT_CONTROL_PROTOCOL.GetOneShotBootMode()

### Summary

Gets the hardware-triggered one-shot boot mode.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_BOOT_CONTROL_GET_ONE_SHOT_BOOT_MODE)(
  IN GBL_EFI_BOOT_CONTROL_PROTOCOL *Self,
  OUT GBL_EFI_ONE_SHOT_BOOT_MODE   *Mode
  );
```

### Related Definitions

#### GBL_EFI_ONE_SHOT_BOOT_MODE

```c
enum {
  GBL_EFI_ONE_SHOT_BOOT_MODE_NONE = 0,
  GBL_EFI_ONE_SHOT_BOOT_MODE_BOOTLOADER,
  GBL_EFI_ONE_SHOT_BOOT_MODE_RECOVERY,
};

typedef UINT32 GBL_EFI_ONE_SHOT_BOOT_MODE;
```

### Parameters

#### Self

A pointer to the `GBL_EFI_BOOT_CONTROL_PROTOCOL` instance.

#### Mode

An output parameter that will contain the overriding boot mode. See
[GBL_EFI_ONE_SHOT_BOOT_MODE][one_shot_boot_mode] for possible values.

### Description

Devices often define custom mechanisms (e.g., hardware key combinations like
"volume down" during power-on) to determine whether to enter Bootloader or
Recovery mode.

This method checks whether such a hardware combination is triggered and returns
the corresponding one-shot boot mode.

Note: This method should **only** return the boot mode triggered by physical
hardware interactions. It must not check boot mode metadata stored in persistent
storage.

### Status Codes Returned

| Return Code             | Semantics                        |
| :---------------------- | :------------------------------- |
| `EFI_SUCCESS`           | The call completed successfully. |
| `EFI_INVALID_PARAMETER` | `Self` or `Mode` is `NULL`.      |

## GBL_EFI_BOOT_CONTROL_PROTOCOL.HandleLoadedOs()

### Summary

Handles loaded OS images and allows for overriding the OS entry point logic.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_BOOT_CONTROL_HANDLE_LOADED_OS)(
  IN GBL_EFI_BOOT_CONTROL_PROTOCOL *Self,
  IN CONST GBL_EFI_LOADED_OS       *Os
  );
```

### Parameters

#### Self

A pointer to the `GBL_EFI_BOOT_CONTROL_PROTOCOL` instance.

#### Os

A pointer to a `GBL_EFI_LOADED_OS` structure representing the loaded OS images.
The underlying images are guaranteed to remain at the same physical addresses
across this call and the default GBL hand-off logic. The `Os` pointer itself is
only valid for the duration of this call and must not be retained.

### Related Definitions

#### GBL_EFI_LOADED_OS

```c
typedef struct _GBL_EFI_LOADED_OS {
  UINTN                KernelSize;
  EFI_PHYSICAL_ADDRESS Kernel;
  UINTN                RamdiskSize;
  EFI_PHYSICAL_ADDRESS Ramdisk;
  UINTN                DeviceTreeSize;
  EFI_PHYSICAL_ADDRESS DeviceTree;
  UINT64               Reserved[8];
} GBL_EFI_LOADED_OS;
```

##### KernelSize

The size of the provided kernel image in bytes.

##### Kernel

The physical memory address of the loaded kernel image.

##### RamdiskSize

The size of the provided ramdisk image in bytes.

##### Ramdisk

The physical memory address of the loaded ramdisk image.

##### DeviceTreeSize

The size of the provided device tree image in bytes.

##### DeviceTree

The physical memory address of the loaded device tree image.

##### Reserved

Reserved for potential future use.

### Description

This method allows the firmware to handle OS images after they have been loaded
by GBL. It can be used to inspect the final kernel, ramdisk, and device tree
images to finalize internal state or perform additional verification steps.

Additionally, the firmware implementation may override the HLOS handoff by
performing device-specific hardware preparation and executing the kernel jump
logic directly, without returning control to GBL. In this case, the firmware is
responsible for calling `ExitBootServices()`. If the method returns, GBL
proceeds with its default handoff logic.

GBL guarantees that this method is always executed in the `TPL_APPLICATION`
context.

Implementation is optional. Returning either `EFI_SUCCESS` or `EFI_UNSUPPORTED`
has the same effect: the boot process continues.

### Status Codes Returned

| Return Code              | Semantics                                                                                |
| :----------------------- | :--------------------------------------------------------------------------------------- |
| `EFI_SUCCESS`            | OS images were handled successfully.                                                     |
| `EFI_UNSUPPORTED`        | The firmware does not need to handle OS images. GBL continues the boot process.          |
| `EFI_INVALID_PARAMETER`  | `Self` or `Os` is `NULL`. GBL will fail to boot.                                         |
| `EFI_SECURITY_VIOLATION` | The provided OS images fail to meet device security requirements. GBL will fail to boot. |
| `EFI_DEVICE_ERROR`       | An internal error occurred while handling OS images. GBL will fail to boot.              |

[get_slot_count]: #gbl_efi_boot_control_protocol_getslotcount
[get_slot_info]: #gbl_efi_boot_control_protocol_getslotinfo
[get_current_slot]: #gbl_efi_boot_control_protocol_getcurrentslot
[set_active_slot]: #gbl_efi_boot_control_protocol_setactiveslot
[get_one_shot_boot_mode]: #gbl_efi_boot_control_protocol_getoneshotbootmode
[handle_loaded_os]: #gbl_efi_boot_control_protocol_handleloadedos
[custom_protocol_revisions]: efi_integration.md#gbl-custom-protocol-revisions
[slot_info]: #gbl_efi_slot_info
[one_shot_boot_mode]: #gbl_efi_one_shot_boot_mode
[ab_boot_flow]: ./gbl_ab_boot_flow.md
