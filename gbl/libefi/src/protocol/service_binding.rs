// Copyright 2025, The Android Open Source Project
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

//! Rust wrapper for `EFI_SERVICE_BINDING_PROTOCOL`.

use crate::{
    efi_call,
    protocol::{Protocol, ProtocolInfo},
    DeviceHandle, EfiEntry,
};
use core::mem::ManuallyDrop;
use efi_types::{EfiGuid, EfiHandle, EfiServiceBindingProtocol};
use liberror::Result;

/// Trait for coupling a binding protocol to the protocol
/// used by the application or driver.
///
/// See https://uefi.org/specs/UEFI/2.11/11_Protocols_UEFI_Driver_Model.html#efi-service-binding-protocol
/// for more information.
pub trait ServiceBindingProtocolInfo {
    const GUID: EfiGuid;

    type ChildProtocol: ProtocolInfo;
}

impl<SBP: ServiceBindingProtocolInfo> ProtocolInfo for SBP {
    const GUID: EfiGuid = SBP::GUID;

    type InterfaceType = EfiServiceBindingProtocol;
}

impl<SBP: ServiceBindingProtocolInfo> Protocol<'_, SBP> {
    /// Creates a child handle for SBP::ChildProtocol.
    pub(super) fn create_child(&self) -> Result<DeviceHandle> {
        let mut handle: EfiHandle = core::ptr::null_mut();

        // SAFETY:
        // `self.interface()?` guarantees self.interface is non-null and points to a valid object
        // established by `Protocol::new()`.
        // `self.interface` is an input parameter and will not be retained. It outlives the call.
        // `handle` is an output parameter and will not be retained. It outlives the call.
        unsafe { efi_call!(self.interface()?.create_child, self.interface, &mut handle)? };

        Ok(DeviceHandle(handle))
    }

    /// Destroys the child handle.
    ///
    /// SAFETY:
    ///
    /// * It is the caller's responsibility to verify that `handle` was created
    ///   by a call to `self.create_child`.
    /// * It is the caller's responsibility to verify that `destroy_child` is called
    ///   at most once per handle.
    pub(super) unsafe fn destroy_child(&self, handle: DeviceHandle) -> Result<()> {
        // SAFETY:
        // * `self.interface()?` guarantees self.interface is non-null and points to a
        //   valid object established by `Protocol::new()`.
        // * `self.interface` is an input parameter and will not be retained.
        //   It outlives the call.
        unsafe { efi_call!(self.interface()?.destroy_child, self.interface, handle.0) }
    }
}

/// Wrapper around ServiceBindingProtocol to manage
/// the lifetime of the child device handle and child protocol.
pub struct ProtocolBinder<'a, SBP: ServiceBindingProtocolInfo> {
    service_protocol: Protocol<'a, SBP>,
    // At some point the `Drop` impl for `Protocol` will call `close_protocol` to clean up.
    // In this instance, the child handle is managed by the Service Binding Protocol,
    // so we want to bypass the call to `close_protocol`.
    child_protocol: ManuallyDrop<Protocol<'a, SBP::ChildProtocol>>,
}

impl<'a, SBP: ServiceBindingProtocolInfo> ProtocolBinder<'a, SBP> {
    /// Creates a new ProtocolBinder, wrapping the service protocol,
    /// child handle, and child protocol.
    pub fn new(entry: &'a EfiEntry) -> Result<Self> {
        let service_protocol = entry.system_table().boot_services().find_first_and_open::<SBP>()?;
        let child_handle = service_protocol.create_child()?;
        let child_protocol = entry
            .system_table()
            .boot_services()
            .handle_protocol::<SBP::ChildProtocol>(child_handle)
            .map(ManuallyDrop::new)
            // Not fully instantiated yet, so manually destroy child handle
            // if opening the child protocol failed.
            .inspect_err(|_| {
                // Safety:
                // * `child_handle` was just created by `service_protocol.create_child()`
                // * No other copies of `child_handle` exist.
                let _ = unsafe { service_protocol.destroy_child(child_handle) };
            })?;

        Ok(Self { service_protocol, child_protocol })
    }

    /// Returns a reference to the child protocol instance.
    pub fn child_protocol(&self) -> &Protocol<'a, SBP::ChildProtocol> {
        &self.child_protocol
    }
}

impl<'a, SBP: ServiceBindingProtocolInfo> Drop for ProtocolBinder<'a, SBP> {
    fn drop(&mut self) {
        // Safety:
        // * `self.child_protocol.device` was created by a call to
        //   `self.service_protocol.create_child()`.
        // * `self` owns the device handle and this is the only call to `destroy_child`;
        //   there are no other open protocols on the device handle.
        let _ = unsafe { self.service_protocol.destroy_child(self.child_protocol.device) };
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use efi_types::{
        EfiHandle, EfiStatus, EFI_STATUS_INVALID_PARAMETER, EFI_STATUS_SUCCESS,
        EFI_STATUS_UNSUPPORTED,
    };
    use std::{cell::RefCell, collections::VecDeque};

    /// A structure to store the traces of the CreateChild method call.
    #[derive(Default)]
    pub struct CreateChildTrace {
        pub outputs: VecDeque<(EfiHandle, EfiStatus)>,
    }

    /// A structure to store traces of the DestroyChild method call.
    #[derive(Default)]
    pub struct DestroyChildTrace {
        pub inputs: VecDeque<EfiHandle>,
    }

    /// A structure to store traces of the service binding protocol.
    #[derive(Default)]
    pub struct ServiceBindingCallTraces {
        pub create_child_trace: CreateChildTrace,
        pub destroy_child_trace: DestroyChildTrace,
    }

    thread_local! {
        static SERVICE_BINDING_CALL_TRACES: RefCell<ServiceBindingCallTraces> = RefCell::new(Default::default());
    }

    /// Accessor for service
    pub fn service_binding_call_traces(
    ) -> &'static std::thread::LocalKey<RefCell<ServiceBindingCallTraces>> {
        &SERVICE_BINDING_CALL_TRACES
    }

    pub fn reinitialized_service_binding_call_traces(
    ) -> &'static std::thread::LocalKey<RefCell<ServiceBindingCallTraces>> {
        SERVICE_BINDING_CALL_TRACES.with(|traces| {
            *traces.borrow_mut() = Default::default();
        });

        service_binding_call_traces()
    }

    pub extern "efiapi" fn create_child(
        _: *mut EfiServiceBindingProtocol,
        child_handle: *mut EfiHandle,
    ) -> EfiStatus {
        service_binding_call_traces().with(|traces| {
            let create_child = &mut traces.borrow_mut().create_child_trace;
            let Some((handle, status)) = create_child.outputs.pop_front() else {
                return EFI_STATUS_UNSUPPORTED;
            };
            if child_handle.is_null() {
                EFI_STATUS_INVALID_PARAMETER
            } else {
                // SAFETY:
                // * `child_handle` is not null.
                unsafe { *child_handle = handle };
                status
            }
        })
    }

    pub extern "efiapi" fn destroy_child(
        _: *mut EfiServiceBindingProtocol,
        child_handle: EfiHandle,
    ) -> EfiStatus {
        service_binding_call_traces().with(|traces| {
            let destroy_child = &mut traces.borrow_mut().destroy_child_trace;
            destroy_child.inputs.push_back(child_handle);

            EFI_STATUS_SUCCESS
        })
    }
}
