# GBL EFI Fastboot Transport Protocol

This document describes the GBL Fastboot Transport protocol. The protocol
defines interfaces that can be used by EFI applications to implement Fastboot
device side logic.

|             |            |
| ----------- | ---------- |
| **Status**  | Pre-frozen |
| **Created** | 2024-3-21  |

## GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL

### Summary

This protocol defines interfaces that abstracts platform Fastboot transport
implementation such as USB/TCP or other custom channels.

### GUID

```c
// {edade92c-5c48-440d-849c-e2a0c7e55143}
#define GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL_GUID           \
  {                                                  \
    0xedade92c, 0x5c48, 0x440d, {                    \
      0x84, 0x9c, 0xe2, 0xa0, 0xc7, 0xe5, 0x51, 0x43 \
    }                                                \
  }
```

### Revision Number

```c
#define GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL_REVISION GBL_PROTOCOL_REVISION(0, 256)
```

See [GBL Custom Protocol Revisions][custom_protocol_revisions] for details about
protocol revisions.

### Protocol Interface Structure

```c
typedef struct _GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL {
  UINT64                                     Revision;
  CONST CHAR8                                *Description;
  GBL_EFI_FASTBOOT_TRANSPORT_INTERFACE_START Start;
  GBL_EFI_FASTBOOT_TRANSPORT_INTERFACE_STOP  Stop;
  GBL_EFI_FASTBOOT_TRANSPORT_RECEIVE         Receive;
  GBL_EFI_FASTBOOT_TRANSPORT_SEND            Send;
  GBL_EFI_FASTBOOT_TRANSPORT_FLUSH           Flush;
} GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL;
```

### Parameters

#### Revision

The revision to which the GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL adheres. All
future revisions must be backwards compatible. If a future version is not
backwards compatible, a different GUID must be used.

#### Description

A static null-terminated ASCII string that describes the transport.

#### Start

Starts the transport for Fastboot traffic. See
[`GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.Start()`][start].

#### Stop

Stops the transport. See [`GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.Stop()`][stop].

#### Receive

Receives data from the transport if available. See
[`GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.Receive()`][receive].

#### Send

Sends data to the transport. See
[`GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.Send()`][send].

#### Flush

Flushes and waits for all pending sends to complete. See
[`GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.Flush()`][flush].

## GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.Start()

### Summary

Start the transport channel.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_FASTBOOT_TRANSPORT_INTERFACE_START)(
  IN GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL *Self
  );
```

### Parameters

#### Self

A pointer to the [`GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL`][protocol] instance.

### Description

The function is responsible for allocating necessary resources and setting up
the transport channel.

### Status Codes Returned

| Return Code           | Semantics                              |
| :-------------------- | :------------------------------------- |
| `EFI_SUCCESS`         | Transport is started successfully.     |
| `EFI_ALREADY_STARTED` | The transport is already started.      |
| `EFI_DEVICE_ERROR`    | The physical device reported an error. |

## GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.Stop()

### Summary

Stops the transport interface.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_FASTBOOT_TRANSPORT_INTERFACE_STOP)(
  IN GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL *Self
  );
```

### Parameters

#### Self

A pointer to the [`GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL`][protocol] instance.

### Description

The function should abort pending transfers, deallocate resources and stop the
transport channel.

### Status Codes Returned

| Return Code        | Semantics                              |
| :----------------- | :------------------------------------- |
| `EFI_SUCCESS`      | Transport is stopped successfully.     |
| `EFI_NOT_STARTED`  | The transport is not started.          |
| `EFI_DEVICE_ERROR` | The physical device reported an error. |

## GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.Receive()

### Summary

Receives data from the transport channel

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_FASTBOOT_TRANSPORT_RECEIVE)(
  IN GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL *Self,
  IN OUT UINTN                           *BufferSize,
  OUT VOID                               *Buffer,
  IN GBL_EFI_FASTBOOT_RX_MODE            Mode
  );
```

### Parameters

#### Self

A pointer to the [`GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL`][protocol] instance.

#### BufferSize

On entry, the value may represent size of `Buffer` or total requested data size.
See description below for more details. On exit, it is set to the size in bytes
of the data that was received.

#### Buffer

A pointer to the data buffer to receive data.

#### Mode

Indicates whether to receive a single packet of any size or multiple packets
until a fixed number of bytes.

### Description

`Receive()` should poll and, if available, receive available data into the
provided buffer.

If `Mode` is set to `SINGLE_PACKET`. Implementation should return on any
fastboot packet received. If provided buffer is not enough to read the received
data, implementation should return `EFI_BUFFER_TOO_SMALL` and `BufferSize`
should be set to the required buffer size.

If `Mode` is set to `FIXED_LENGTH`, caller should set `BufferSize` to the total
amount data expected to be received from the transport. Partial receive is
allowed. Implementation does not need to block until buffer is fully filled.
Caller should be prepared to call this API again with updated size for remaining
data.

`Buffer` is caller allocated and managed. Implementation should store the
received data in internal buffer first and then copy to `Buffer`. To improve
performance, if `Mode` is set to `FIXED_LENGTH` it is recommended that before
copying data to user, implementation first initiates the receive for the next
fastboot packet asynchronously based on remaining data size (i.e. input
`BufferSize` minus newly received size), so that the two operations can be done
in parallel.

Note: For USB implementation, some host implementation (i.e. upstream Android)
does not send zero-length-packet to indicate transaction completion, which may
cause host driver to hold the delivery of the final USB packet if it is not a
short packet and device requests more than actual amount of data. To avoid this,
it is recommended that when `Mode` is `SINGLE_PACKET`, implementation should
only queue single USB packet request, and when `Mode` is `FIXED_LENGTH`, the USB
request size should be no more than the remaining data size.

When using the GBL EFI application, in the event of disconnection/reconnection
(i.e. unplug/plug of USB, host fastboot application abort), the safest option is
to reboot the device to start from a clean state. If that is not desired, driver
can perform the following to recover within the same session:

1. Returns EFI_DEVICE_ERROR at least once following disconnection. GBL will
   abort current command and wait for the next command.
2. Handle the reconnection transparently within this API, so that GBL can
   continue to read data from the transport.

Note: If disconnection/reconnection occurs during data phase (i.e. fastboot
fetch), there may be left over data on the USB bus that will be picked up by
future fastboot commands. This can lead to wrong result. It is recommended for
the host to drain/reset USB bus before issuing new command.

### Related Definitions

```c
enum {
  GBL_EFI_FASTBOOT_RX_MODE_SINGLE_PACKET,
  GBL_EFI_FASTBOOT_RX_MODE_FIXED_LENGTH,
};
typedef UINT32 GBL_EFI_FASTBOOT_RX_MODE;
```

#### SINGLE_PACKET

Receives a single packet of any size

#### FIXED_LENGTH

Receives multiple packets until a fixed number of bytes.

### Status Codes Returned

| Return Code             | Semantics                                            |
| :---------------------- | :--------------------------------------------------- |
| `EFI_SUCCESS`           | Read is successful                                   |
| `EFI_INVALID_PARAMETER` | A parameter is invalid.                              |
| `EFI_BUFFER_TOO_SMALL`  | The provided buffer is too small for available data. |
| `EFI_NOT_STARTED`       | The transport is not started.                        |
| `EFI_NOT_READY`         | No data available from the transport.                |
| `EFI_DEVICE_ERROR`      | The physical device reported an error.               |

## GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.Send()

### Summary

Sends data to the transport channel.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_FASTBOOT_TRANSPORT_SEND)(
  IN GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL *Self,
  IN OUT UINTN                           *BufferSize,
  IN CONST VOID                          *Buffer
  );
```

### Parameters

#### Self

A pointer to the [`GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL`][protocol] instance.

#### BufferSize

On entry, the size in bytes of `Buffer` to be sent. On exit, the size in bytes
of the data that was actually queued or sent.

#### Buffer

A pointer to the data buffer to be sent.

### Description

`Buffer` is caller allocated and managed. Implementation should make an internal
copy of the portion of data actually queued.

The function does not need to be blocking and can return immediately once the
data is queued for transfer. If implementation does not have available resource
to serve the request, i.e. queue is full, `EFI_NOT_READY` should be returned.

If using the GBL EFI application, See
[`GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.Receive()`][receive] for recommended
handling of disconnection/reconnection events if full device reset is not
desired.

### Status Codes Returned

| Return Code             | Semantics                                          |
| :---------------------- | :------------------------------------------------- |
| `EFI_SUCCESS`           | Some data is sent successfully.                    |
| `EFI_INVALID_PARAMETER` | A parameter is invalid.                            |
| `EFI_NOT_STARTED`       | The transport is not started.                      |
| `EFI_NOT_READY`         | The driver is not ready to queue or send new data. |
| `EFI_DEVICE_ERROR`      | The physical device reported an error.             |

## GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.Flush()

### Summary

Waits until all pending TX transfers are completed.

### Prototype

```c
typedef
EFI_STATUS
(EFIAPI *GBL_EFI_FASTBOOT_TRANSPORT_INTERFACE_FLUSH)(
  IN GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL *Self
  );
```

### Parameters

#### Self

A pointer to the [`GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL`][protocol] instance.

### Description

The function should wait until all data sent via `Send()` either arrives at the
remote endpoint or is sent out to the transport. To avoid hanging due to
unresponsive host/device, implementation may use timeout internally, in which
case `EFI_TIMEOUT` should be returned if this happens.

### Status Codes Returned

| Return Code        | Semantics                              |
| :----------------- | :------------------------------------- |
| `EFI_SUCCESS`      | Flush completed successfully.          |
| `EFI_NOT_STARTED`  | The transport is not started.          |
| `EFI_TIMEOUT`      | Timeout waiting for send to complete.  |
| `EFI_DEVICE_ERROR` | The physical device reported an error. |

[start]: #gbl_efi_fastboot_transport_protocol_start
[stop]: #gbl_efi_fastboot_transport_protocol_stop
[receive]: #gbl_efi_fastboot_transport_protocol_receive
[send]: #gbl_efi_fastboot_transport_protocol_send
[flush]: #gbl_efi_fastboot_transport_protocol_flush
[protocol]: #gbl_efi_fastboot_transport_protocol
[custom_protocol_revisions]: efi_integration.md#gbl-custom-protocol-revisions
