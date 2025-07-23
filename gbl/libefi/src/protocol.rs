// Copyright 2023, The Android Open Source Project
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

//! EFI protocol wrappers to provide Rust-safe APIs for usage.

use crate::{DeviceHandle, EfiEntry};
use core::{
    ops::{Deref, DerefMut},
    ptr::null_mut,
};
use efi_types::{defs::EfiGuid, protocol::Client, Identified};
use liberror::{Error, Result};

pub mod block_io;
pub mod block_io2;
pub mod device_path;
pub mod dt_fixup;
pub mod erase_block;
pub mod gbl_efi_ab_slot;
pub mod gbl_efi_avb;
pub mod gbl_efi_avf;
pub mod gbl_efi_boot_memory;
pub mod gbl_efi_fastboot;
pub mod gbl_efi_fastboot_transport;
pub mod gbl_efi_image_loading;
pub mod gbl_efi_os_configuration;
pub mod loaded_image;
pub mod random_number_generator;
pub mod riscv;
pub mod simple_network;
pub mod simple_text_input;
pub mod simple_text_output;
pub mod timestamp;

pub(super) mod hash2;
pub(crate) mod service_binding;

/// Describes whether a Protocol is required or optional.
pub enum Requirement {
    /// The protocol is a mandatory requirement for supporting GBL.
    Mandatory,
    /// The protocol is an optional requirement for supporting GBL.
    Optional,
}

/// ProtocolInfo provides GUID info and the EFI data structure type for a protocol.
pub trait ProtocolInfo {
    /// Data structure type of the interface.
    type InterfaceType;
    /// GUID of the protocol.
    const GUID: EfiGuid;
    /// Whether the protocol is mandatory or optional.
    const REQUIREMENT: Requirement = Requirement::Mandatory;
}

/// Temporary trait to abstract over protocols using [ProtocolInfo] vs [Client].
/// Once we use [Client] everywhere this can go away.
pub trait ProtocolImpl {
    /// The raw C struct type.
    type CInterface;
    /// The underlying implementation type.
    type ImplType;
    /// The protocol GUID.
    const GUID: EfiGuid;
    /// Whether the protocol is mandatory or optional.
    const REQUIREMENT: Requirement = Requirement::Mandatory;

    /// Creates the corresponding `ImplType` from a raw C struct.
    ///
    /// # Safety
    ///
    /// * `c_interface` must point to a valid `CInterface` object
    /// * `c_interface` must outlive the returned `ImplType`
    /// * ownership of `c_interface` must be passed in, and must not be used
    ///   again except through the returned `ImplType`
    unsafe fn new_impl(c_interface: *mut Self::CInterface) -> Self::ImplType;
}

/// For [ProtocolInfo], the implementation type is a raw C struct pointer.
impl<T: ProtocolInfo> ProtocolImpl for T {
    type CInterface = T::InterfaceType;
    type ImplType = *mut T::InterfaceType;
    const GUID: EfiGuid = T::GUID;
    const REQUIREMENT: Requirement = T::REQUIREMENT;

    unsafe fn new_impl(c_interface: *mut Self::CInterface) -> Self::ImplType {
        // Just pass the c_interface pointer through, we use it directly.
        c_interface
    }
}

/// For [Client], the implementation is a [Client] itself.
impl<T: Identified> ProtocolImpl for Client<T> {
    type CInterface = T;
    type ImplType = Self;
    const GUID: EfiGuid = T::GUID;

    unsafe fn new_impl(c_interface: *mut Self::CInterface) -> Self::ImplType {
        // SAFETY: by function safety,
        // * `c_interface` is a valid `CInterface`
        // * `c_interface` will outlive the returned `ImplType`
        // * we have exclusive ownership of `c_interface`, which we transfer
        //   into `Client` without retaining a copy
        unsafe { Client::new(c_interface) }
    }
}

/// A generic type for representing an EFI protcol.
pub struct Protocol<'a, T: ProtocolImpl> {
    // The handle to the device offering the protocol. It's needed for closing the protocol.
    device: DeviceHandle,
    // The protocol implementation.
    interface: T::ImplType,
    // The `EfiEntry` data
    efi_entry: &'a EfiEntry,
}

/// Common functions for Protocol<T> with either raw or [Client] backend.
///
/// Protocol<T> may have additional implementation based on type `T`.
impl<'a, T: ProtocolImpl> Protocol<'a, T> {
    /// Create a new instance with the given device handle, interface pointer and `EfiEntry` data.
    ///
    /// # Safety
    ///
    /// * `c_interface` must point to a valid `T::CInterface` object
    /// * `c_interface` must outlive the returned `Protocol`
    /// * ownership of `c_interface` must be passed in, and must not be used
    ///   again except through the returned `Protocol`
    pub(crate) unsafe fn new(
        device: DeviceHandle,
        c_interface: *mut T::CInterface,
        efi_entry: &'a EfiEntry,
    ) -> Self {
        // SAFETY: by function safety,
        // * `c_interface` is a valid `T::CInterface`
        // * `c_interface` will outlive the returned `Protocol`
        // * we have exclusive ownership of `c_interface`, which we transfer
        //   into `T::new_impl` without retaining a copy
        let interface = unsafe { T::new_impl(c_interface) };
        Self { device, interface, efi_entry }
    }

    /// Returns the reference to EFI entry.
    pub fn efi_entry(&self) -> &'a EfiEntry {
        self.efi_entry
    }
}

/// Additional functions for Protocol<T> with a raw pointer implementation.
impl<'a, T: ProtocolInfo> Protocol<'a, T> {
    /// Returns the EFI data structure for the protocol interface.
    pub fn interface(&self) -> Result<&T::InterfaceType> {
        // SAFETY: EFI protocol interface data structure.
        unsafe { self.interface.as_ref() }.ok_or(Error::InvalidInput)
    }

    /// Returns the mutable pointer of the interface. Invisible from outside. Application should
    /// not have any need to alter the content of interface data.
    pub(crate) fn interface_ptr(&self) -> *mut T::InterfaceType {
        self.interface
    }
}

/// Protocol<T> with a [Client] implementation can deref to [Client] to call
/// its protocol APIs.
impl<'a, T: Identified> Deref for Protocol<'a, Client<T>> {
    type Target = Client<T>;

    fn deref(&self) -> &Self::Target {
        &self.interface
    }
}

/// Protocol<T> with a [Client] implementation can deref to [Client] to call
/// its protocol APIs.
impl<'a, T: Identified> DerefMut for Protocol<'a, Client<T>> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.interface
    }
}

impl<T: ProtocolImpl> Drop for Protocol<'_, T> {
    fn drop(&mut self) {
        // If the device handle is not specified when creating the Protocol<T>, treat the
        // handle as a static permanent reference and don't close it. An example is
        // `EFI_SYSTEM_TABLE.ConOut`.
        if self.device.0 != null_mut() {
            // Currently we open all protocols using flags BY_HANDLE_PROTOCOL. The flag allows a
            // protocol to be opened for multiple copies, which is needed if a UEFI protocol
            // implementation also require access for other protocols. But if any one of them is
            // closed, all other opened copies will be affected. Therefore for now we don't close
            // the protocol on drop. In the future when we start using other flags such as
            // EXCLUSIVE, we should perform protocol close based on the open flags.

            // self.efi_entry.system_table().boot_services().close_protocol::<T>(self.device).unwrap();
        }
    }
}

/// Macro to perform an EFI protocol function call.
///
/// In the first variant, the first argument is the function pointer,
/// and the following arguments are passed through as protocol args.
///
/// With our [Protocol] struct, usage generally looks something like:
///
/// ```
/// efi_call!(
///   self.interface()?.protocol_function_name,
///   self.interface,
///   arg1,
///   arg2,
///   ...
/// )
/// ```
/// Most efi_call! invocations should use the first variant.
///
/// With the second variant, the first argument is an expression that references
/// a buffer in-out size parameter.
/// This is part of a pattern used by some protocol methods
/// that take an output buffer and an in-out buffer size:
/// if the method returns EFI_STATUS_BUFFER_TOO_SMALL,
/// the size is mutated to contain the minimum required buffer size.
/// The caller can then allocate a larger buffer and reattempt the method call.
///
/// Usage generally looks something like:
/// ```
/// efi_call!(
///   @bufsize arg2,
///   self.interface()?.protocol_function_name,
///   self.interface,
///   arg1,
///   &mut arg2,
///   ...
/// )
/// ```
#[macro_export]
macro_rules! efi_call {
    ( $method:expr, $($x:expr),*$(,)? ) => {
        {
            use liberror::{Error, Result, efi_status_to_result};
            use libutils::{method_basename, func_name};
            let res: Result<()> = match $method {
                None => {
                    $crate::efi_try_print!("Protocol method not found in caller '{}': {}\r\n",
                                           func_name!(),
                                           method_basename(stringify!($method)));
                    Err(Error::NotFound)
                },
                Some(f) => efi_status_to_result(f($($x,)*)),
            };
            res
        }
    };
    ( @bufsize $size:expr, $method:expr, $($x:expr),*$(,)? ) => {
        {
            use liberror::{Error, Result, efi_status_to_result};
            use efi_types::EFI_STATUS_BUFFER_TOO_SMALL;
            use libutils::{method_basename, func_name};
            let res: Result<()> = match $method {
                None => {
                    $crate::efi_try_print!("Protocol method not found in caller '{}': {}\r\n",
                                           func_name!(),
                                           method_basename(stringify!($method)));
                    Err(Error::NotFound)},
                Some(f) => {
                    match f($($x,)*) {
                        EFI_STATUS_BUFFER_TOO_SMALL => Err(Error::BufferTooSmall(Some($size))),
                        r => efi_status_to_result(r),
                    }
                },
            };
            res
        }
    };
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test::*;
    use efi_types::defs::EfiBlockIoProtocol;

    #[test]
    fn test_dont_close_protocol_without_device_handle() {
        run_test(|image_handle, systab_ptr| {
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let mut block_io: EfiBlockIoProtocol = Default::default();
            // SAFETY: `block_io` is a EfiBlockIoProtocol and out lives the created Protocol.
            unsafe {
                Protocol::<block_io::BlockIoProtocol>::new(
                    DeviceHandle(null_mut()),
                    &mut block_io as *mut _,
                    &efi_entry,
                );
            }
            efi_call_traces().with(|traces| {
                assert_eq!(traces.borrow_mut().close_protocol_trace.inputs.len(), 0);
            });
        })
    }
}
