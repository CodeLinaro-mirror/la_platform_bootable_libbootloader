// Copyright 2026, The Android Open Source Project
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

//! Rust trait for
//! [`GBL_EFI_AVF_PROTOCOL`](https://android.googlesource.com/platform/bootable/libbootloader/+/refs/heads/gbl-mainline/gbl/docs/gbl_efi_avf_protocol.md).

use core::slice;

use crate::{
    defs::{EfiGuid, EfiStatus, GblEfiAvfProtocol},
    protocol::{BridgeToRust, Provider},
    status::{EfiError, EfiResult},
    Identified,
};

/// Protocol Rust API for `GBL_EFI_AVF_PROTOCOL`.
#[cfg_attr(feature = "mocks", mockall::automock)]
pub trait GblEfiAvf {
    /// Returns the protocol revision that the implementation is providing.
    fn revision(&self) -> u64;

    /// Implementation of `GBL_EFI_AVF_PROTOCOL.ReadVendorDiceHandover`.
    ///
    /// # Returns
    ///
    /// * `Ok(size)` - the number of bytes written into `buffer`.
    /// * `Err(EfiError::BufferTooSmall(required))` - `buffer` is too small; `required` is the
    ///   size needed to hold the handover.
    fn read_vendor_dice_handover(&self, buffer: &mut [u8]) -> EfiResult<usize>;

    /// Implementation of `GBL_EFI_AVF_PROTOCOL.ReadSecretKeeperPublicKey`.
    ///
    /// # Returns
    ///
    /// * `Ok(size)` - the number of bytes written into `buffer`.
    /// * `Err(EfiError::BufferTooSmall(required))` - `buffer` is too small; `required` is the
    ///   size needed to hold the key.
    /// * `Err(EfiError::Unsupported)` - this firmware provides no Secretkeeper public key. GBL
    ///   omits the corresponding device tree property in that case. Do not report this by
    ///   returning `Ok(0)`, which GBL reads as a present-but-empty key.
    fn read_secretkeeper_public_key(&self, buffer: &mut [u8]) -> EfiResult<usize>;
}

impl Identified for GblEfiAvfProtocol {
    const GUID: EfiGuid =
        EfiGuid::new(0xe7f1c4a6, 0x0a52, 0x4f61, [0xbd, 0x98, 0x9e, 0x60, 0xb5, 0x59, 0x45, 0x2a]);
}

// SAFETY:
// * our wrapper functions adhere to the protocol spec
unsafe impl<R: GblEfiAvf> BridgeToRust<R> for GblEfiAvfProtocol {
    unsafe fn create_bridge(rust_impl: &R) -> Self {
        Self {
            revision: rust_impl.revision(),
            read_vendor_dice_handover: Some(Provider::<_, R>::read_vendor_dice_handover),
            read_secretkeeper_public_key: Some(Provider::<_, R>::read_secretkeeper_public_key),
        }
    }
}

impl<R: GblEfiAvf> Provider<'_, GblEfiAvfProtocol, R> {
    /// Implementation of `GBL_EFI_AVF_PROTOCOL.ReadVendorDiceHandover`.
    ///
    /// # Safety
    ///
    /// * Caller must guarantee that `interface` is a valid pointer to a
    ///   `GblEfiAvfProtocol` struct inside a `Self` struct.
    /// * Caller must guarantee that `handover_size` points to a valid `usize` parameter.
    /// * If `*handover_size > 0`, caller must guarantee that `handover` points to a valid
    ///   buffer of at least `*handover_size` bytes.
    unsafe extern "efiapi" fn read_vendor_dice_handover(
        interface: *mut GblEfiAvfProtocol,
        handover_size: *mut usize,
        handover: *mut u8,
    ) -> EfiStatus {
        // SAFETY: By safety contract, `interface` is a valid pointer to a
        // `GblEfiAvfProtocol` struct inside a `Self` struct. Since it comes as the
        // first field, `interface` is also a valid pointer to the `Self` struct.
        let rust_impl = unsafe { Self::to_rust(interface) };
        (|| {
            // SAFETY: By safety contract, `handover_size` points to a valid parameter.
            let size = unsafe { handover_size.as_mut().ok_or(EfiError::InvalidParameter)? };
            if *size > 0 && handover.is_null() {
                return Err(EfiError::InvalidParameter);
            }
            let buf = match *size {
                0 => &mut [],
                // SAFETY: By safety contract, `handover` points to a buffer of at least
                // `*size` bytes.
                len => unsafe { slice::from_raw_parts_mut(handover, len) },
            };
            match rust_impl.read_vendor_dice_handover(buf) {
                rep @ Ok(used) | rep @ Err(EfiError::BufferTooSmall(used)) => {
                    *size = used;
                    rep
                }
                v => v,
            }
        })()
        .into()
    }

    /// Implementation of `GBL_EFI_AVF_PROTOCOL.ReadSecretKeeperPublicKey`.
    ///
    /// # Safety
    ///
    /// * Caller must guarantee that `interface` is a valid pointer to a
    ///   `GblEfiAvfProtocol` struct inside a `Self` struct.
    /// * Caller must guarantee that `key_size` points to a valid `usize` parameter.
    /// * If `*key_size > 0`, caller must guarantee that `key` points to a valid buffer of
    ///   at least `*key_size` bytes.
    unsafe extern "efiapi" fn read_secretkeeper_public_key(
        interface: *mut GblEfiAvfProtocol,
        key_size: *mut usize,
        key: *mut u8,
    ) -> EfiStatus {
        // SAFETY: By safety contract, `interface` is a valid pointer to a
        // `GblEfiAvfProtocol` struct inside a `Self` struct. Since it comes as the
        // first field, `interface` is also a valid pointer to the `Self` struct.
        let rust_impl = unsafe { Self::to_rust(interface) };
        (|| {
            // SAFETY: By safety contract, `key_size` points to a valid parameter.
            let size = unsafe { key_size.as_mut().ok_or(EfiError::InvalidParameter)? };
            if *size > 0 && key.is_null() {
                return Err(EfiError::InvalidParameter);
            }
            let buf = match *size {
                0 => &mut [],
                // SAFETY: By safety contract, `key` points to a buffer of at least `*size` bytes.
                len => unsafe { slice::from_raw_parts_mut(key, len) },
            };
            match rust_impl.read_secretkeeper_public_key(buf) {
                rep @ Ok(used) | rep @ Err(EfiError::BufferTooSmall(used)) => {
                    *size = used;
                    rep
                }
                v => v,
            }
        })()
        .into()
    }
}
