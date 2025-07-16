// Copyright 2024, The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// This EFI application implements a demo for booting Android/Fuchsia from disk. See
// bootable/libbootloader/gbl/README.md for how to run the demo. See comments of
// `android_boot:android_boot_demo()` and `fuchsia_boot:fuchsia_boot_demo()` for
// supported/unsupported features at the moment.

use crate::utils::{get_platform_buffer_info, BufferInfo, SZ_MB};
use alloc::{boxed::Box, vec::Vec};
use core::{future::Future, mem::take, pin::Pin, str::from_utf8, time::Duration};
use efi::{
    efi_println,
    local_session::LocalFastbootSession,
    protocol::{gbl_efi_fastboot_transport::GblFastbootTransportProtocol, Protocol},
    EfiEntry, WatchdogTimerCode,
};
use efi_types::GBL_IMAGE_TYPE_FASTBOOT;
use fastboot::{Transport, MAX_COMMAND_SIZE};
use gbl_async::{poll, YieldCounter};
use liberror::{Error, Result};
use libgbl::{
    fastboot::{GblTcpStream, GblUsbTransport, PinFutContainer, TcpStream},
    {android_boot::GblFastbootEntry, GblOps},
};

const FASTBOOT_WATCHDOG_TIMER_CODE: WatchdogTimerCode = WatchdogTimerCode::new(0x10000);

// Network fastboot is only allowed on dev boards.
#[cfg(feature = "gbl_dev")]
mod network_fastboot {
    use super::*;
    pub(super) use crate::net::{EfiGblNetwork, EfiTcpSocket};
    pub(super) use core::sync::atomic::AtomicU64;

    const FASTBOOT_TCP_PORT: u16 = 5554;
    const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

    #[derive(PartialEq)]
    /// Represents the connection state resolution done by previous `accept_new` call.
    enum AcceptNewState {
        /// First call of `accept_new`. No previous state.
        None,
        /// A connection was successfully accepted by a previous `accept_new` call.
        Accepted,
        /// We're actively listening for a new connection.
        Listening,
    }

    pub(super) struct EfiFastbootTcpTransport<'a, 'b, 'c> {
        socket: &'c mut EfiTcpSocket<'a, 'b>,
        accept_new_state: AcceptNewState,
    }

    impl<'a, 'b, 'c> EfiFastbootTcpTransport<'a, 'b, 'c> {
        pub(super) fn new(socket: &'c mut EfiTcpSocket<'a, 'b>) -> Self {
            Self { socket: socket, accept_new_state: AcceptNewState::None }
        }
    }

    impl TcpStream for EfiFastbootTcpTransport<'_, '_, '_> {
        /// Reads to `out` for exactly `out.len()` number bytes from the TCP connection.
        async fn read_exact(&mut self, out: &mut [u8]) -> Result<()> {
            self.socket.receive_exact(out, DEFAULT_TIMEOUT).await
        }

        /// Sends exactly `data.len()` number bytes from `data` to the TCP connection.
        async fn write_exact(&mut self, data: &[u8]) -> Result<()> {
            self.socket.send_exact(data, DEFAULT_TIMEOUT).await
        }
    }

    impl GblTcpStream for EfiFastbootTcpTransport<'_, '_, '_> {
        fn accept_new(&mut self) -> bool {
            let efi_entry = self.socket.efi_entry;
            self.socket.poll();
            self.accept_new_state = match self.accept_new_state {
                // If first call or connection has been accepted before, just reset to establish a new
                // connection.
                AcceptNewState::None | AcceptNewState::Accepted => {
                    let _ = self
                        .socket
                        .listen(FASTBOOT_TCP_PORT)
                        .inspect_err(|e| efi_println!(efi_entry, "TCP listen error: {:?}", e));

                    // TODO(b/368647237): Enable only in Fuchsia context.
                    self.socket.broadcast_fuchsia_fastboot_mdns();
                    AcceptNewState::Listening
                }
                AcceptNewState::Listening => {
                    // If new connection is accepted, mark and return indication of it.
                    // If it's been `DEFAULT_TIMEOUT`, restart listening in case the remote client
                    // disconnects in the middle of TCP handshake and leaves the socket in a half open
                    // state.
                    if self.socket.is_established() {
                        self.socket.set_io_yield_threshold(1024 * 1024); // 1MB
                        let remote = self.socket.get_socket().remote_endpoint().unwrap();
                        efi_println!(efi_entry, "TCP connection from {}", remote);
                        AcceptNewState::Accepted
                    } else if self.socket.time_since_last_listen() > DEFAULT_TIMEOUT {
                        let _ = self
                            .socket
                            .listen(FASTBOOT_TCP_PORT)
                            .inspect_err(|e| efi_println!(efi_entry, "TCP listen error: {:?}", e));

                        // TODO(b/368647237): Enable only in Fuchsia context.
                        self.socket.broadcast_fuchsia_fastboot_mdns();
                        AcceptNewState::Listening
                    } else {
                        AcceptNewState::Listening
                    }
                }
            };

            self.accept_new_state == AcceptNewState::Accepted
        }
    }
}

/// No-op network fastboot types for prod builds.
///
/// [GblFastbootEntry::run] takes an `Option<impl GblTcpStream>` argument, and
/// unfortunately the Rust compiler does seem to require a real [GblTcpStream]
/// implementation even if we're just passing `None` (since it needs to know how
/// much stack space to reserve when instantiating the function).
///
/// Creating some no-op types that will never actually be used is the simplest
/// solution; we could modify the fastboot library to omit TCP support entirely
/// in prod builds but it would be a much larger refactor, and more conditional
/// compilation generally means less readable code.
///
/// Once never-type is stable, we might be able to delete this workaround and
/// use `None::<!>` instead to represent a TCP connection in prod builds.
#[cfg(not(feature = "gbl_dev"))]
mod network_fastboot {
    use super::*;

    pub(super) struct NoOpTcp {}

    impl TcpStream for NoOpTcp {
        async fn read_exact(&mut self, _out: &mut [u8]) -> Result<()> {
            unimplemented!();
        }

        async fn write_exact(&mut self, _data: &[u8]) -> Result<()> {
            unimplemented!();
        }
    }

    impl GblTcpStream for NoOpTcp {
        fn accept_new(&mut self) -> bool {
            unimplemented!();
        }
    }
}

use network_fastboot::*;

/// `UsbTransport` implements the `fastboot::Transport` trait using USB interfaces from
/// GBL_EFI_FASTBOOT_USB_PROTOCOL.
struct UsbTransport<'a> {
    protocol: Protocol<'a, GblFastbootTransportProtocol>,
    io_yield_counter: YieldCounter,
    // Buffer for prefetching an incoming packet in `wait_for_packet()`.
    // Alternatively we can also consider adding an EFI event for packet arrive. But UEFI firmware
    // may be more complicated.
    prefetched: (Vec<u8>, usize),
}

impl<'a> UsbTransport<'a> {
    fn new(protocol: Protocol<'a, GblFastbootTransportProtocol>) -> Self {
        Self {
            protocol,
            io_yield_counter: YieldCounter::new(1024 * 1024),
            prefetched: (vec![0u8; MAX_COMMAND_SIZE + 1], 0),
        }
    }

    /// Polls and cache the next USB packet.
    ///
    /// Returns Ok(true) if there is a new packet. Ok(false) if there is no incoming packet. Err()
    /// otherwise.
    fn poll_next_packet(&mut self) -> Result<bool> {
        match &mut self.prefetched {
            (pkt, len) if *len == 0 => match self.protocol.receive(pkt, true) {
                Ok(out_size) => {
                    *len = out_size;
                    return Ok(true);
                }
                Err(Error::NotReady) => return Ok(false),
                Err(e) => return Err(e),
            },
            _ => Ok(true),
        }
    }
}

impl Transport for UsbTransport<'_> {
    async fn receive_packet(&mut self, out: &mut [u8]) -> Result<usize> {
        let len = match &mut self.prefetched {
            (pkt, len) if *len > 0 => {
                let out = out.get_mut(..*len).ok_or(Error::BufferTooSmall(Some(*len)))?;
                let src = pkt.get(..*len).ok_or(Error::Other(Some("Invalid USB read size")))?;
                out.clone_from_slice(src);
                take(len)
            }
            _ => self.protocol.receive_packet(out, out.len() <= MAX_COMMAND_SIZE).await?,
        };
        // Forces a yield to the executor if the data received/sent reaches a certain
        // threshold. This is to prevent the async code from holding up the CPU for too long
        // in case IO speed is high and the executor uses cooperative scheduling.
        self.io_yield_counter.increment(len.try_into().unwrap()).await;
        Ok(len)
    }

    /// Sends data over the USB channel.
    async fn send_packet(&mut self, packet: &[u8]) -> Result<()> {
        self.protocol.send_all(packet).await
    }
}

impl GblUsbTransport for UsbTransport<'_> {
    fn has_packet(&mut self) -> bool {
        let efi_entry = self.protocol.efi_entry();
        self.poll_next_packet()
            .inspect_err(|e| efi_println!(efi_entry, "Error while polling next packet: {:?}", e))
            .unwrap_or(false)
    }
}

impl Drop for UsbTransport<'_> {
    fn drop(&mut self) {
        let entry = self.protocol.efi_entry();
        let _ = self
            .protocol
            .stop()
            .inspect_err(|e| efi_println!(entry, "Fail to stop transport: {e}"));
    }
}

/// Initializes the Fastboot USB interface and returns a `UsbTransport`.
fn init_usb(efi_entry: &EfiEntry) -> Result<UsbTransport> {
    let protocol = efi_entry
        .system_table()
        .boot_services()
        .find_first_and_open::<GblFastbootTransportProtocol>()?;
    match protocol.stop() {
        Err(e) if e != Error::NotStarted => Err(e),
        _ => {
            protocol.start()?;
            Ok(UsbTransport::new(protocol))
        }
    }
}

// Wrapper of vector of pinned futures.
#[derive(Default)]
struct VecPinFut<'a>(Vec<Pin<Box<dyn Future<Output = ()> + 'a>>>);

impl<'a> PinFutContainer<'a> for VecPinFut<'a> {
    fn add_with<F: Future<Output = ()> + 'a>(&mut self, f: impl FnOnce() -> F) {
        self.0.push(Box::pin(f()));
    }

    fn for_each_remove_if(
        &mut self,
        mut cb: impl FnMut(&mut Pin<&mut (dyn Future<Output = ()> + 'a)>) -> bool,
    ) {
        for idx in (0..self.0.len()).rev() {
            cb(&mut self.0[idx].as_mut()).then(|| self.0.swap_remove(idx));
        }
    }
}

/// Helper for handling fastboot mode.
pub(crate) fn efi_gbl_fastboot_entry<'a, 'b, G: GblOps<'a, 'b>>(
    entry: &EfiEntry,
    fb: GblFastbootEntry<'_, G>,
    fastboot_buffer_info: &mut Option<BufferInfo>,
) {
    let img_type_fastboot = from_utf8(GBL_IMAGE_TYPE_FASTBOOT).unwrap();
    // Checks if we have a reserved buffer for fastboot
    // Note: `get_or_insert_with` lazily evaluates closure (only when insert is necessary).
    let buffer = fastboot_buffer_info
        .get_or_insert_with(|| get_platform_buffer_info(&entry, img_type_fastboot, 512 * SZ_MB));
    let mut alloc;
    let buffer = match buffer {
        BufferInfo::Static(v) => &mut v[..],
        BufferInfo::Alloc(sz) => {
            alloc = vec![0u8; *sz];
            efi_println!(entry, "Allocated {:#x} bytes for fastboot buffer.", alloc.len());
            &mut alloc
        }
    };

    let local = LocalFastbootSession::start(entry, Duration::from_millis(1))
        .inspect(|_| efi_println!(entry, "Starting local bootmenu."))
        .inspect_err(|e| efi_println!(entry, "Failed to start local bootmenu: {:?}", e))
        .ok();

    let usb = init_usb(entry)
        .inspect(|v| efi_println!(entry, "Started Fastboot Channel: {}.", v.protocol.description()))
        .inspect_err(|e| efi_println!(entry, "Failed to start Fastboot over USB. {:?}.", e))
        .ok();

    #[cfg(not(feature = "gbl_dev"))]
    let tcp: Option<NoOpTcp> = None;

    // This is super ugly, but the lifetimes require these objects to remain
    // on the stack so we can't put it all in a single scope gated behind the
    // "gbl_dev" feature. Instead each statement has to be individually gated.
    #[cfg(feature = "gbl_dev")]
    let ts = AtomicU64::new(0);
    #[cfg(feature = "gbl_dev")]
    let mut net: EfiGblNetwork = Default::default();
    #[cfg(feature = "gbl_dev")]
    let mut tcp = net
        .init(entry, &ts)
        .inspect(|v| {
            efi_println!(entry, "Started Fastboot over TCP");
            efi_println!(entry, "IP address:");
            v.interface().ip_addrs().iter().for_each(|v| {
                efi_println!(entry, "\t{}", v.address());
            });
        })
        .inspect_err(|e| efi_println!(entry, "Failed to start EFI network. {:?}.", e))
        .ok();
    #[cfg(feature = "gbl_dev")]
    let tcp = tcp.as_mut().map(|v| EfiFastbootTcpTransport::new(v));

    if local.is_some() || usb.is_some() || tcp.is_some() {
        let _ = entry
            .system_table()
            .boot_services()
            .set_watchdog_timer(Duration::ZERO, FASTBOOT_WATCHDOG_TIMER_CODE)
            .inspect(|_| efi_println!(entry, "Watchdog is successfully reset for fastboot."))
            .inspect_err(|e| efi_println!(entry, "Failed to reset watchdog. {:?}.", e));
    }

    // We currently only consider 1 parallel flash + 1 parallel download.
    // This can be made configurable if necessary.
    const GBL_FB_N: usize = 2;
    let mut bufs = Vec::from_iter(buffer.chunks_exact_mut(buffer.len() / GBL_FB_N));
    let bufs = &(&mut bufs[..]).into();
    let mut fut = Box::pin(fb.run(bufs, VecPinFut::default(), local, usb, tcp));
    while poll(&mut fut).is_none() {}
}
