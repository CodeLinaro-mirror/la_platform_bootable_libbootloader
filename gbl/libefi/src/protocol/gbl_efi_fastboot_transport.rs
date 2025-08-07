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

//! Rust wrapper for `GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL`.

use crate::{
    efi_call,
    protocol::{Protocol, ProtocolInfo, Requirement},
};
use efi_types::{
    EfiGuid, GblEfiFastbootRxMode, GblEfiFastbootTransportProtocol,
    GBL_EFI_FASTBOOT_RX_MODE_FIXED_LENGTH, GBL_EFI_FASTBOOT_RX_MODE_SINGLE_PACKET,
};
use gbl_async::yield_now;
use liberror::{Error, Result};

/// GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL
pub struct GblFastbootTransportProtocol;

impl ProtocolInfo for GblFastbootTransportProtocol {
    type InterfaceType = GblEfiFastbootTransportProtocol;

    const GUID: EfiGuid =
        EfiGuid::new(0xedade92c, 0x5c48, 0x440d, [0x84, 0x9c, 0xe2, 0xa0, 0xc7, 0xe5, 0x51, 0x43]);

    const REQUIREMENT: Requirement = Requirement::Optional;
}

/// Wrapper for GblEfiFastbootRxMode
#[derive(Copy, Clone, Debug)]
pub enum ReceiveMode {
    /// Receive a single packet
    SinglePacket,
    /// Receive a fixed number of bytes
    FixedLength,
}

impl From<ReceiveMode> for GblEfiFastbootRxMode {
    fn from(mode: ReceiveMode) -> Self {
        match mode {
            ReceiveMode::SinglePacket => GBL_EFI_FASTBOOT_RX_MODE_SINGLE_PACKET,
            ReceiveMode::FixedLength => GBL_EFI_FASTBOOT_RX_MODE_FIXED_LENGTH,
        }
    }
}

// Protocol interface wrappers.
impl Protocol<'_, GblFastbootTransportProtocol> {
    /// Wrapper of `GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.start()`
    pub fn start(&self) -> Result<()> {
        // SAFETY:
        // `self.interface()?` guarantees self.interface is non-null and points to a valid object
        // established by `Protocol::new()`.
        // `self.interface` and `max_packet_size` are input/output parameters, outlive the call and
        // will not be retained.
        unsafe { efi_call!(self.interface()?.start, self.interface) }
    }

    /// Wrapper of `GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.stop()`
    pub fn stop(&self) -> Result<()> {
        // SAFETY:
        // `self.interface()?` guarantees self.interface is non-null and points to a valid object
        // established by `Protocol::new()`.
        // `self.interface` is input parameter, outlives the call, and will not be retained.
        unsafe { efi_call!(self.interface()?.stop, self.interface) }
    }

    /// Wrapper of `GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.receive()`
    pub fn receive(&self, out: &mut [u8], mode: ReceiveMode) -> Result<usize> {
        let mut out_size = out.len();
        // SAFETY:
        // `self.interface()?` guarantees self.interface is non-null and points to a valid object
        // established by `Protocol::new()`.
        // `self.interface`, `out_size` and `buffer` are input/output parameters, outlive the call
        // and will not be retained.
        unsafe {
            efi_call!(
                @bufsize out_size,
                self.interface()?.receive,
                self.interface,
                &mut out_size,
                out.as_mut_ptr() as _,
                mode.into(),
            )?;
        }

        Ok(out_size)
    }

    /// Wrapper of `GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.send()`
    pub fn send(&self, data: &[u8]) -> Result<usize> {
        let mut out_size = data.len();
        // SAFETY:
        // `self.interface()?` guarantees self.interface is non-null and points to a valid object
        // established by `Protocol::new()`.
        // `self.interface`, `out_size` and `buffer` are input/output parameters, outlive the call
        // and will not be retained.
        unsafe {
            efi_call!(
                @bufsize out_size,
                self.interface()?.send,
                self.interface,
                &mut out_size,
                data.as_ptr() as _,
            )?;
        }

        Ok(out_size)
    }

    /// Wrapper of `GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.flush()`
    pub fn flush(&self) -> Result<()> {
        // SAFETY:
        // `self.interface()?` guarantees self.interface is non-null and points to a valid object
        // established by `Protocol::new()`.
        unsafe { efi_call!(self.interface()?.flush, self.interface) }
    }

    /// Wrapper of `GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.description()`
    pub fn description(&self) -> &'static str {
        let Ok(s) = self.interface().map(|v| v.description) else { return "" };
        // SAFETY: By UEFI spec, `f` returns a static NULL terminated ASCII string.
        unsafe { core::ffi::CStr::from_ptr(s as _).to_str().unwrap() }
    }

    /// Receives the next packet from the USB.
    pub async fn receive_packet(&self, out: &mut [u8], mode: ReceiveMode) -> Result<usize> {
        loop {
            match self.receive(out, mode) {
                Ok(out_size) => return Ok(out_size),
                Err(Error::NotReady) => yield_now().await,
                Err(e) => return Err(e),
            }
        }
    }

    /// Sends `data` over the USB
    ///
    /// # Args
    ///
    /// * `data`: Data to send. Can be more than one packet of data.
    pub async fn send_all(&self, mut data: &[u8]) -> Result<()> {
        while !data.is_empty() {
            match self.send(data) {
                Err(Error::NotReady) => yield_now().await,
                Err(e) => return Err(e),
                Ok(sz) => data = &data[sz..],
            }
        }
        self.flush()
    }

    /// Wraps `GBL_EFI_FASTBOOT_TRANSPORT_PROTOCOL.revision()`.
    pub fn revision(&self) -> Result<u64> {
        Ok(self.interface()?.revision)
    }
}
