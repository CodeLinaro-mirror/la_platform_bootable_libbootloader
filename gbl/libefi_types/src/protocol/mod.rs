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

//! Rust UEFI protocols.
//!
//! This module provides Rust traits for protocols used by GBL. The protocol
//! APIs are designed to mirror the underlying UEFI protocol definitions, but
//! modified to be more idiomatic Rust, for example:
//!
//! * buffers are slices rather than (pointer, length) pairs
//! * output params are converted to return values instead when feasible
//! * return values are `Result`s rather than raw status enum
//!
//! These protocol traits are usable both on the client side (UEFI application)
//! and the provider side (UEFI firmware). GBL will use the client-side APIs,
//! but devices are not required to use the provider-side APIs, they are just
//! offered as an option to make it easier to build GBL-compatible UEFI firmware
//! in Rust.
//!
//! ```text
//!        GBL           Protocol API           Rust backend (optional)
//!  ---------------------------------------------------------------------
//!                                           Rust protocol provider
//!                                <----------
//!                        C struct
//!               <--------
//!    Rust client
//!
//!    Make Rust calls
//!               -------->
//!                        C struct
//!                                ---------->
//!                                           Rust provider implementation
//! ```
//!
//! This library does not provide a full UEFI firmware implementation, it only
//! aims to wrap protocol pointers. How those pointers get into and out of the
//! UEFI stack is up to the particular implementation.
//!
//! # Client usage
//!
//! Protocol clients such as GBL will get the protocol C pointer from UEFI,
//! then wrap them in a [Client] object, which exposes protocol functionality
//! via safe Rust APIs.
//!
//! # Provider usage
//!
//! Protocol providers such as the UEFI firmware or a driver will implement
//! the protocol Rust backend, then pass it into a [Provider], which exposes
//! a raw C pointer to register with the UEFI stack. Calls on this C pointer
//! will route back into the Rust backend implementation.
//!
//! Additionally there is a [BridgeToRust] trait that [Provider] requires, which
//! properly initializes the C struct to pass calls back into the Rust backend.
//! This will be provided for protocols supported by this library, but can also
//! be implemented manually for new protocols or if users want to define a
//! different Rust API to wrap an existing protocol.

pub mod block_io;

use crate::Identified;

/// A UEFI protocol client.
///
/// This wraps the protocol C struct and exposes the Rust API, so that the
/// protocol can be used with as little unsafe code as possible.
///
/// Usage:
///
/// 1. Open a protocol to get the C struct
/// 2. Call [Client::new] to wrap the C struct
/// 3. Make calls on the [Client] API
/// 4. Drop the [Client]
/// 5. Close the protocol
pub struct Client<C: 'static + Identified>(&'static mut C);

impl<C: 'static + Identified> Client<C> {
    /// Creates a new protocol client.
    ///
    /// # Arguments
    ///
    /// * `c_interface`: the protocol implementation C pointer.
    ///
    /// # Panics
    ///
    /// If `c_interface` is null.
    ///
    /// # Safety
    ///
    /// * `c_interface` must be a valid object adhering to the UEFI spec
    /// * `c_interface` must outlive the returned [Client]
    /// * the caller must pass ownership of `c_interface` and not retain a copy
    ///
    /// In a UEFI application, this means the returned [Client] must be dropped
    /// before calling `CloseProtocol()` or `ExitBootServices()`
    pub unsafe fn new(c_interface: *mut C) -> Self {
        // SAFETY:
        // * function safety requires a valid `c_interface` which outlives us
        // * function safety requires we own the only copy
        let c_interface = unsafe { c_interface.as_mut() }.unwrap();
        Self(c_interface)
    }
}

/// A UEFI protocol provider.
///
/// This wraps the Rust API and exposes the protocol C struct, so that calls on
/// the struct route back to the Rust implementation.
///
/// Usage:
///
/// 1. Implement a Rust protocol trait
/// 2. Call [Provider::new] to wrap the Rust implementation
/// 3. Call [Provider::to_ptr] to get the C struct
/// 4. Register the C struct with the UEFI protocol database
#[repr(C)]
pub struct Provider<'a, C: Identified, R> {
    // This must come first, so that with `repr(C)` we can cast between this
    // `c_interface` pointer and the overall `Provider` pointer.
    c_interface: C,
    rust_impl: &'a mut R,
}

impl<'a, C: 'a + Identified + BridgeToRust<R>, R> Provider<'a, C, R> {
    /// Creates a new [Provider] backed by the given Rust implementation.
    pub fn new(rust_impl: &'a mut R) -> Self {
        // SAFETY: we hold a borrow of `rust_impl` so we know it will not be
        // moved or destroyed while we exist.
        let c_interface = unsafe { C::create_bridge(rust_impl) };
        Self { c_interface, rust_impl }
    }
}

impl<'a, C: 'a + Identified, R> Provider<'a, C, R> {
    /// Returns the raw UEFI C interface pointer backed by this provider.
    ///
    /// # Arguments
    ///
    /// * `self`: the [Provider] reference
    ///
    /// ## Lifetimes
    ///
    /// The `self` borrow lifetime `'a` here is the same as our capture of the
    /// Rust backend, which means that this function borrows `self` for the
    /// remainder of its lifetime. The purpose of this is to get compile-time
    /// enforcement that `self` won't be used or moved after this call:
    ///
    /// ```compile_fail
    /// # use efi_types::{Identified, protocol::Provider};
    ///
    /// fn to_ptr_twice<'a, C: 'a + Identified, R>(provider: &'a mut Provider<'a, C, R>) {
    ///   unsafe { provider.to_ptr() };
    ///
    ///   // The second call will fail since `provider` was permanently
    ///   // borrowed the first time.
    ///   unsafe { provider.to_ptr() };
    /// }
    /// ```
    ///
    /// However, this lifetime does not prevent `self` from being dropped,
    /// so it's still up to the caller to keep it alive while the pointer
    /// exists. See the safety docs for more details.
    ///
    /// # Returns
    ///
    /// A pointer which can be registered with the UEFI protocol database and
    /// handed out to clients. When clients make C calls on this struct, it
    /// will automatically route back into the Rust backing implementation.
    ///
    /// # Safety
    ///
    /// The returned pointer must be treated as a mutable borrow of `self`.
    /// In particular:
    ///
    /// * `self` must outlive the returned pointer
    /// * there must be only one copy of the pointer in use at a time
    ///
    /// For UEFI this means the resulting protocol pointer cannot be given out
    /// to multiple clients. This is because when a client calls into the
    /// protocol, it converts back to a `&mut` reference, and having multiple
    /// `&mut` to the same backing provider would violate the Rust ownership
    /// model and cause undefined behavior.
    ///
    /// It is OK to give the pointer to clients at different times, e.g. if the
    /// first client closes the protocol the pointer can then be handed out to
    /// another client. But if multiple clients might open a protocol
    /// concurrently, there must be a unique provider object for each client
    /// (though the providers may potentially then use interior mutability or
    /// synchronization techniques to share a common backend). If this becomes
    /// burdensome, it might be possible to integrate interior mutability into
    /// this library, but for now we're keeping it simple.
    ///
    /// Lastly, this also requires any client calling into the returned pointer
    /// to adhere to the protocol definitions and requirements. Clients using
    /// this Rust library to call into the protocols have pretty strong safety
    /// guarantees, with the only unsafe code being the Rust <-> C <-> Rust
    /// bridge layer. Other clients, particulary those written in C, could cause
    /// undefined behavior if they violate the protocol requirements.
    pub unsafe fn to_ptr(&'a mut self) -> *mut C {
        &mut self.c_interface as *mut C
    }

    /// Converts the raw C interface pointer back to our Rust implementation.
    ///
    /// # Safety
    ///
    /// `this` must be the C interface pointer returned by [to_ptr()], which
    /// guarantees by its safety requirements that the object will exist for
    /// `'a` and we have exclusive access to it.
    ///
    /// Most commonly this condition will be satisfied by using the first
    /// argument of a UEFI protocol function, which is the C interface pointer.
    pub(crate) unsafe fn to_rust(this: *mut C) -> &'a mut R {
        // `repr(C)` lets us cast between the struct and its first item.
        let this = this as *mut Self;
        // SAFETY:
        // Function safety requires `this` is the `to_ptr()` pointer. By
        // `to_ptr()` safety docs and implementation, we know:
        // * `this` points to a valid non-null `Provider`
        // * `this` represents exclusive access of the `Provider`, so we can
        //   safety convert it to `&mut`
        unsafe { this.as_mut() }.unwrap().rust_impl
    }
}

/// Bridges a C protocol interface to a Rust backend implementation.
///
/// This trait gets implemented on the protocol C struct to configure it to
/// forward calls into a Rust provider backend `R`.
///
/// # Safety
///
/// The created C struct must adhere to the UEFI protocol spec - pointers must
/// be valid, function implementations must obey requirements, etc.
///
/// Pointers require particular care, e.g. if the protocol structure contains
/// something like `UINT8* data`. These pointers will be set once upon bridge
/// creation and must remain valid for the lifetime of the bridge.
///
/// The Rust backend will stay valid and unmoved while the bridge exists, so the
/// recommended way to approach this is to put the data inside the Rust struct,
/// something like this:
///
/// ```
/// use efi_types::protocol::BridgeToRust;
///
/// // The C protocol definition.
/// struct FooC {
///   data: *mut u8
/// }
///
/// // The corresponding Rust API.
/// trait FooRust {
///   fn data(&mut self) -> &mut [u8];
/// }
///
/// struct FooImpl {
///   // Data lives inside our struct, so as long as the struct can't move
///   // the data won't either.
///   //
///   // Note that this is NOT necessarily the case if `data` was something like
///   // `Vec<u8>` that lives outside the struct - in that case you would have
///   // to ensure the vector never reallocates once the [Provider] is created.
///   data: [u8; 8]
/// }
///
/// impl FooRust for FooImpl {
///   fn data(&mut self) -> &mut [u8] {
///     &mut self.data
///   }
/// }
///
/// // SAFETY: we can assign `rust_impl.data()` to a pointer because `rust_impl`
/// // is guaranteed to stay valid an unmoved while the bridge exists, and the
/// // data lives inside `rust_impl`.
/// unsafe impl BridgeToRust<FooImpl> for FooC {
///   unsafe fn create_bridge(rust_impl: &mut FooImpl) -> Self {
///     Self { data: rust_impl.data().as_mut_ptr() as *mut _ }
///   }
/// }
/// ```
///
/// Otherwise, it is up to the implementation to ensure that pointers remain
/// valid and safe to convert back to a reference according to
/// https://doc.rust-lang.org/std/ptr/index.html#pointer-to-reference-conversion.
pub unsafe trait BridgeToRust<R> {
    /// Creates the C struct that will forward calls into `rust_impl`.
    ///
    /// # Safety
    ///
    /// This function may create pointers from `self` into `rust_impl`, so to
    /// ensure those pointers stay valid, `rust_impl` must remain valid and
    /// unmoved until `self` is dropped.
    unsafe fn create_bridge(rust_impl: &mut R) -> Self;
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use crate::{
        defs::{EfiGuid, EfiStatus},
        status::{EfiError, EfiResult},
    };
    use mockall::automock;
    use std::{ffi::c_void, marker::PhantomData, ops::Range, slice};

    /// Returns a buffer pointer ranges.
    ///
    /// The `==` operator on slices only compares contents, but for our UEFI
    /// protocols we often want to be sure that a buffer is passed exactly
    /// through a protocol without any intermediate copies or pointer mixups.
    ///
    /// This also converts to `usize` which makes it easier to use with
    /// mockall expectations.
    pub fn buffer_range<T>(buffer: &[T]) -> Range<usize> {
        let range = buffer.as_ptr_range();
        // Convert to usize because mockall objects require `Send` which
        // raw pointers are not. There are some workarounds but converting
        // here makes usage easier.
        Range { start: range.start as usize, end: range.end as usize }
    }

    #[test]
    fn same_buffer_true() {
        let buffer = [1, 2, 3, 4];
        let buffer_ref = &buffer;
        assert_eq!(buffer_range(&buffer), buffer_range(buffer_ref));
    }

    #[test]
    fn same_buffer_false() {
        let buffer = [1, 2, 3, 4];
        let buffer2 = [1, 2, 3, 4];
        assert_ne!(buffer_range(&buffer), buffer_range(&buffer2));
    }

    /// Fake protocol C interface for testing.
    ///
    /// Declares a few differently-shaped functions and data fields to test that
    /// they can be properly modeled in our library. This can also be used as a
    /// reference for adding new protocols.
    ///
    /// Normally this would be defined in C and bindgen would convert it to
    /// Rust, but here we just define the Rust struct directly for simplicity.
    #[repr(C)]
    struct CInterface {
        /// A raw value.
        pub value: u64,

        /// A pointer to some data that lives outside the struct.
        /// This must be a valid pointer.
        pub external_data: *mut [u8; 8],

        /// A function that sends a const buffer (e.g. to write to disk).
        pub send_buffer: Option<
            unsafe extern "efiapi" fn(
                self_: *mut Self,
                buffer_size: usize,
                buffer: *const c_void,
            ) -> EfiStatus,
        >,

        /// A function that sends a mut buffer (e.g. to read from disk).
        pub send_buffer_mut: Option<
            unsafe extern "efiapi" fn(
                self_: *mut Self,
                buffer_size: usize,
                buffer: *mut c_void,
            ) -> EfiStatus,
        >,
    }

    impl Identified for CInterface {
        const GUID: EfiGuid = EfiGuid::new(0, 1, 2, [3, 4, 5, 6, 7, 8, 9, 10]);
    }

    /// Fake protocol Rust API wrapper.
    ///
    /// The API of the Rust wrapper is up to us, but it should mirror the C API
    /// pretty closely with just some modified argument and return types to use
    /// more idiomatic Rust.
    ///
    /// # Safety
    ///
    /// The reference returned by [external_data] must not be invalidated by any
    /// other protocol methods.
    ///
    /// This is necessary because the C struct holds a pointer to the data. See
    /// [BridgeToRust] safety docs for details on pointers in protocols.
    #[automock]
    unsafe trait RustInterface {
        /// Returns `value`.
        fn value(&mut self) -> u64;

        /// Returns `external_data` as a const ref.
        // The explicit lifetimes are required for `automock`, if we try to
        // elide them it gives a compile error.
        fn external_data<'a>(&'a mut self) -> EfiResult<&'a [u8; 8]>;

        /// Rust API for `send_buffer`.
        fn send_buffer(&mut self, buffer: &[u8]) -> EfiResult<()>;

        /// Rust API for `send_buffer_mut`.
        fn send_buffer_mut(&mut self, buffer: &mut [u8]) -> EfiResult<()>;
    }

    /// SAFETY:
    /// * our wrapper functions adhere to the protocol spec
    /// * [RustInterface] safety guarantees the [external_data] pointer stays
    ///   valid
    unsafe impl<R: RustInterface> BridgeToRust<R> for CInterface {
        unsafe fn create_bridge(rust_impl: &mut R) -> Self {
            CInterface {
                value: rust_impl.value(),
                external_data: rust_impl.external_data().unwrap() as *const _ as *mut _,
                send_buffer: Some(Provider::<_, R>::send_buffer_wrapper),
                send_buffer_mut: Some(Provider::<_, R>::send_buffer_mut_wrapper),
            }
        }
    }

    /// Provider wrappers to serve the protocol C -> Rust.
    impl<'a, R: RustInterface> Provider<'a, CInterface, R> {
        unsafe extern "efiapi" fn send_buffer_wrapper(
            this: *mut CInterface,
            buffer_size: usize,
            buffer: *const c_void,
        ) -> EfiStatus {
            // SAFETY: UEFI protocols require `this` is our C interface pointer.
            let rust_impl = unsafe { Self::to_rust(this) };
            // SAFETY: protocol spec requires this be a valid buffer.
            let buffer = unsafe { slice::from_raw_parts(buffer as *const u8, buffer_size) };
            rust_impl.send_buffer(buffer).into()
        }

        unsafe extern "efiapi" fn send_buffer_mut_wrapper(
            this: *mut CInterface,
            buffer_size: usize,
            buffer: *mut c_void,
        ) -> EfiStatus {
            // SAFETY: UEFI protocols require `this` is our C interface pointer.
            let rust_impl = unsafe { Self::to_rust(this) };
            // SAFETY: protocol spec requires this be a valid mutable buffer.
            let buffer = unsafe { slice::from_raw_parts_mut(buffer as *mut u8, buffer_size) };
            rust_impl.send_buffer_mut(buffer).into()
        }
    }

    /// Client wrappers to call into the protocol Rust -> C.
    ///
    /// SAFETY: the [external_data] returned reference is never invalidated.
    unsafe impl RustInterface for Client<CInterface> {
        fn value(&mut self) -> u64 {
            self.0.value
        }

        fn external_data(&mut self) -> EfiResult<&[u8; 8]> {
            // SAFETY:
            // * protocol spec guarantees `external_data` pointer is valid
            // * by [Client::new] safety, we are currently the sole owner
            let data = unsafe { self.0.external_data.as_ref() };
            Ok(data.unwrap())
        }

        fn send_buffer(&mut self, buffer: &[u8]) -> EfiResult<()> {
            let func = self.0.send_buffer.unwrap();
            // SAFETY:
            // * `self.0` is only borrowed for the duration of the call
            // * `buffer` will only be read up to `buffer.len()` and is only
            //   borrowed for the duration of the call
            unsafe { func(self.0 as *mut _, buffer.len(), buffer.as_ptr() as *const _) }.into()
        }

        fn send_buffer_mut(&mut self, buffer: &mut [u8]) -> EfiResult<()> {
            let func = self.0.send_buffer_mut.unwrap();
            // SAFETY:
            // * `self.0` is only borrowed for the duration of the call
            // * `buffer` will only be written up to `buffer.len()` and is
            //   only borrowed for the duration of the call
            unsafe { func(self.0 as *mut _, buffer.len(), buffer.as_mut_ptr() as *mut _) }.into()
        }
    }

    const VALUE: u64 = 0xABCD1234;
    const EXTERNAL_DATA: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

    /// Creates a [MockRustInterface] with default expectations set up so that
    /// it can be used as a [Provider].
    fn create_mock() -> MockRustInterface {
        let mut mock = MockRustInterface::new();
        mock.expect_value().return_const(VALUE);
        mock.expect_external_data().return_const(Ok(&EXTERNAL_DATA));
        mock
    }

    /// Connects a [Client] and [Provider] by tunneling through the C interface.
    ///
    /// This properly handles lifetimes and ownership when we control both the
    /// [Client] and [Provider] Rust objects, which lets us omit some `unsafe`
    /// blocks in each test.
    pub(super) struct TestProtocolTunnel<'a, C: 'static + Identified, R> {
        client: Client<C>,
        // This provides compile-time enforcement that the backing [Provider]
        // outlives us. We can't borrow the [Provider] directly because the
        // `to_ptr()` function borrows it instead.
        _phantom_data: PhantomData<&'a mut Provider<'a, C, R>>,
    }

    impl<'a, C: Identified, R> TestProtocolTunnel<'a, C, R> {
        pub(super) fn new(provider: &'a mut Provider<'a, C, R>) -> Self {
            // SAFETY:
            // * `provider` will outlive us due to our `_phantom_data` lifetime
            // * `client` becomes the sole owner of the returned pointer
            let c_interface = unsafe { provider.to_ptr() };

            // SAFETY:
            // * `c_interface` points to a valid UEFI protocol interface
            //   backed by the `provider` implementation
            // * we give exclusive access to the pointer, no copies exist
            let client = unsafe { Client::new(c_interface) };

            Self { client, _phantom_data: PhantomData }
        }

        pub(super) fn client(&mut self) -> &mut Client<C> {
            &mut self.client
        }
    }

    #[test]
    fn value_success() {
        let mut mock = create_mock();
        let mut provider = Provider::new(&mut mock);
        let mut tunnel = TestProtocolTunnel::new(&mut provider);

        assert_eq!(tunnel.client().value(), VALUE);
    }

    #[test]
    fn external_data_success() {
        let mut mock = create_mock();
        let mut provider = Provider::new(&mut mock);
        let mut tunnel = TestProtocolTunnel::new(&mut provider);

        assert_eq!(tunnel.client().external_data(), Ok(&EXTERNAL_DATA));
    }

    #[test]
    fn send_buffer_success() {
        let test_buffer: &[u8] = &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let test_buffer_range = buffer_range(test_buffer);

        let mut mock = create_mock();
        mock.expect_send_buffer().returning(move |buffer: &[u8]| {
            // Make sure we got the exact buffer we expected.
            assert_eq!(buffer_range(buffer), test_buffer_range);
            Ok(())
        });
        let mut provider = Provider::new(&mut mock);
        let mut tunnel = TestProtocolTunnel::new(&mut provider);

        assert_eq!(tunnel.client().send_buffer(test_buffer), Ok(()));
    }

    #[test]
    fn send_buffer_failure() {
        let mut mock = create_mock();
        mock.expect_send_buffer().returning(|_| Err(EfiError::NoMedia));
        let mut provider = Provider::new(&mut mock);
        let mut tunnel = TestProtocolTunnel::new(&mut provider);

        assert_eq!(tunnel.client().send_buffer(&[]), Err(EfiError::NoMedia));
    }

    #[test]
    fn send_buffer_mut_success() {
        let test_buffer: &mut [u8] = &mut [1, 2, 3, 4, 5, 6, 7, 8];
        let test_buffer_range = buffer_range(test_buffer);

        let mut mock = create_mock();
        mock.expect_send_buffer_mut().returning(move |buffer: &mut [u8]| {
            // Make sure we got the exact buffer we expected.
            assert_eq!(buffer_range(buffer), test_buffer_range);
            // Zero the bytes.
            buffer.fill(0);
            Ok(())
        });
        let mut provider = Provider::new(&mut mock);
        let mut tunnel = TestProtocolTunnel::new(&mut provider);

        assert_eq!(tunnel.client().send_buffer_mut(test_buffer), Ok(()));
        assert_eq!(test_buffer, &[0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn send_buffer_mut_failure() {
        let mut mock = create_mock();
        mock.expect_send_buffer_mut().returning(|_| Err(EfiError::NoMedia));
        let mut provider = Provider::new(&mut mock);
        let mut tunnel = TestProtocolTunnel::new(&mut provider);

        assert_eq!(tunnel.client().send_buffer_mut(&mut []), Err(EfiError::NoMedia));
    }
}
