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

//! Rust wrapper for `EFI_IMAGE_LOADING_PROTOCOL`.

use crate::efi_call;
use crate::{
    protocol::{Protocol, ProtocolInfo, Requirement},
    versioned_protocol,
};
use arrayvec::ArrayVec;
use core::mem::{size_of, MaybeUninit};
use efi_types::{
    EfiGuid, GblEfiImageBuffer, GblEfiImageInfo, GblEfiImageLoadingProtocol,
    GBL_EFI_IMAGE_LOADING_PROTOCOL_REVISION,
};
use liberror::{Error, Result};
use spin::Mutex;

/// GBL_IMAGE_LOADING_PROTOCOL
pub struct GblImageLoadingProtocol;

versioned_protocol!(GblImageLoadingProtocol, GBL_EFI_IMAGE_LOADING_PROTOCOL_REVISION);

impl ProtocolInfo for GblImageLoadingProtocol {
    type InterfaceType = GblEfiImageLoadingProtocol;

    const GUID: EfiGuid =
        EfiGuid::new(0xdb84b4fa, 0x53bd, 0x4436, [0x98, 0xa7, 0x4e, 0x02, 0x71, 0x42, 0x8b, 0xa8]);

    const REQUIREMENT: Requirement = Requirement::Optional;
}

/// Max length of a UTF16 partition name in u16 units.
pub const PARTITION_NAME_LEN_U16: usize = efi_types::PARTITION_NAME_LEN_U16 as usize;

/// Max length of a UTF8 partition name in u8 units (bytes).
pub const PARTITION_NAME_LEN_U8: usize = size_of::<char>() * PARTITION_NAME_LEN_U16;

const MAX_ARRAY_SIZE: usize = 100;
static RETURNED_BUFFERS: Mutex<ArrayVec<usize, MAX_ARRAY_SIZE>> = Mutex::new(ArrayVec::new_const());

/// Wrapper class for buffer received with [get_buffer] function.
///
/// Helps to keep track of allocated memory and avoid getting same buffer more than once.
#[derive(Debug)]
pub struct EfiImageBuffer {
    buffer: Option<&'static mut [MaybeUninit<u8>]>,
}

/// Represents either static reserved memory space or memory to be allocated dynamically.
#[derive(Debug)]
pub enum EfiImageBufferInfo {
    /// Static memory space returned from UEFI firmware.
    Buffer(EfiImageBuffer),
    /// Target buffer should be dynamically allocated by the given size.
    AllocSize(usize),
}

impl EfiImageBufferInfo {
    /// Gets as EfiImageBuffer::Buffer;
    pub fn buffer(&mut self) -> Option<&mut [MaybeUninit<u8>]> {
        match self {
            Self::Buffer(EfiImageBuffer { buffer: Some(v) }) => Some(v),
            _ => None,
        }
    }

    /// Move buffer ownership out of EfiImageBuffer, and consume it.
    pub fn take(self) -> Option<&'static mut [MaybeUninit<u8>]> {
        match self {
            Self::Buffer(mut v) => Some(v.take()),
            _ => None,
        }
    }
}

impl EfiImageBuffer {
    // # Safety
    //
    // `gbl_buffer` must represent valid buffer.
    //
    // # Return
    //
    // Err(EFI_STATUS_INVALID_PARAMETER) - If `gbl_buffer.Memory` == NULL
    // Err(EFI_STATUS_ALREADY_STARTED) - Requested buffer was already returned and is still in use.
    // Err(err) - on error
    // Ok(_) - on success
    unsafe fn new(gbl_buffer: GblEfiImageBuffer) -> Result<EfiImageBuffer> {
        if gbl_buffer.Memory.is_null() {
            return Err(Error::InvalidInput);
        }

        let addr = gbl_buffer.Memory as usize;
        let mut returned_buffers = RETURNED_BUFFERS.lock();
        if returned_buffers.contains(&addr) {
            return Err(Error::AlreadyStarted);
        }
        returned_buffers.push(addr);

        // SAFETY:
        // `gbl_buffer.Memory` is guaranteed to be not null
        // This code is relying on EFI protocol implementation to provide valid buffer pointer
        // to memory region of size `gbl_buffer.SizeBytes`.
        Ok(EfiImageBuffer {
            buffer: Some(unsafe {
                core::slice::from_raw_parts_mut(
                    gbl_buffer.Memory as *mut MaybeUninit<u8>,
                    gbl_buffer.SizeBytes,
                )
            }),
        })
    }

    /// Move buffer ownership out of EfiImageBuffer, and consume it.
    pub fn take(&mut self) -> &'static mut [MaybeUninit<u8>] {
        self.buffer.take().unwrap()
    }

    // Removes address from `RETURNED_BUFFERS`.
    //
    // # Safety
    //
    // Caller must guarantee that address is not referenced anymore.
    unsafe fn release(address: usize) {
        let mut returned_buffers = RETURNED_BUFFERS.lock();
        let res = returned_buffers.iter().position(|&val| val == address);
        debug_assert!(
            res.is_some(),
            "EfiImageBuffer::release trying to release address ({address}) that is not tracked"
        );
        if let Some(pos) = res {
            returned_buffers.swap_remove(pos);
        }
    }
}

impl Drop for EfiImageBuffer {
    fn drop(&mut self) {
        if self.buffer.is_none() {
            return;
        }

        // SAFETY:
        // EfiIMageBuffer is the only owner of the buffer. The only way to get address for it is to
        // call `take()` which consumes `self.buffer`, which we check above.
        unsafe { EfiImageBuffer::release((*self.buffer.as_ref().unwrap()).as_ptr() as usize) };
    }
}

// Protocol interface wrappers.
impl Protocol<'_, GblImageLoadingProtocol> {
    /// Wrapper of `GBL_IMAGE_LOADING_PROTOCOL.get_buffer()`
    ///
    /// # Return
    /// Ok(Some(EfiImageBuffer)) if buffer was successfully provided,
    /// Ok(None) if buffer was not provided
    /// Err(Error::EFI_STATUS_BUFFER_TOO_SMALL) if provided buffer is too small
    /// Err(Error::EFI_STATUS_INVALID_PARAMETER) if received buffer is NULL
    /// Err(Error::EFI_STATUS_ALREADY_STARTED) buffer was already returned and is still in use.
    /// Err(err) if `err` occurred
    pub fn get_buffer(&self, gbl_image_info: &GblEfiImageInfo) -> Result<EfiImageBufferInfo> {
        let mut gbl_buffer: GblEfiImageBuffer = Default::default();
        // SAFETY:
        // `self.interface_ptr()` points to a valid object established by `Protocol::new()`.
        // `self.interface_ptr()` and `gbl_buffer` are input/output parameters, outlive the call and
        // will not be retained.
        // `gbl_buffer` returned by this call must not overlap, and will be checked by
        // `EfiImageBuffer`
        unsafe {
            efi_call!(
                @bufsize gbl_image_info.SizeBytes,
                self.interface().get_buffer,
                self.interface_ptr(),
                gbl_image_info,
                &mut gbl_buffer
            )?;
        }

        if gbl_buffer.SizeBytes < gbl_image_info.SizeBytes {
            return Err(Error::BufferTooSmall(Some(gbl_image_info.SizeBytes)));
        } else if gbl_buffer.Memory.is_null() {
            return Ok(EfiImageBufferInfo::AllocSize(gbl_buffer.SizeBytes));
        }

        // SAFETY:
        // `gbl_buffer.Memory` must be not null. This checked in `new()` call
        // `gbl_buffer.Size` must be valid size of the buffer.
        // This protocol is relying on EFI protocol implementation to provide valid buffer pointer
        // to memory region of size `gbl_buffer.SizeBytes`.
        let image_buffer = EfiImageBufferInfo::Buffer(unsafe { EfiImageBuffer::new(gbl_buffer)? });

        Ok(image_buffer)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        protocol::gbl_efi_image_loading::GblImageLoadingProtocol, test::run_test, DeviceHandle,
        EfiEntry,
    };
    use core::{
        ffi::c_void,
        ptr::{from_mut, null_mut, NonNull},
    };
    use efi_types::{EfiStatus, EFI_STATUS_INVALID_PARAMETER, EFI_STATUS_SUCCESS};
    use spin::MutexGuard;
    use std::cell::RefCell;
    use std::collections::HashSet;

    const UCS2_STR: [u16; 8] = [0x2603, 0x0073, 0x006e, 0x006f, 0x0077, 0x006d, 0x0061, 0x006e];
    const UTF8_STR: &str = "☃snowman";

    fn gbl_image_info_from_bytes(data: &[u16]) -> GblEfiImageInfo {
        let mut result = GblEfiImageInfo::default();
        result.ImageType[..data.len()].copy_from_slice(data);
        result
    }

    #[test]
    fn test_image_info_get_type_str() {
        let mut buffer = [0u8; 100];
        // empty string
        assert_eq!(gbl_image_info_from_bytes(&[0u16]).get_type_str(&mut buffer).unwrap(), "");
        assert_eq!(gbl_image_info_from_bytes(&[0u16]).get_type_str(&mut buffer).unwrap(), "");
        assert_eq!(
            gbl_image_info_from_bytes(&[0x0000]).get_type_str(&mut buffer[..0]).unwrap(),
            ""
        );

        // Special characters
        assert_eq!(
            gbl_image_info_from_bytes(&UCS2_STR).get_type_str(&mut buffer).unwrap(),
            UTF8_STR
        );

        // Null character in the middle
        assert_eq!(
            gbl_image_info_from_bytes(&[0x006d, 0x0075, 0x0000, 0x0073, 0x0069, 0x0063])
                .get_type_str(&mut buffer),
            Ok("mu")
        );

        // Null character at the end
        assert_eq!(
            gbl_image_info_from_bytes(&[0x006d, 0x0075, 0x0073, 0x0069, 0x0063, 0x0000])
                .get_type_str(&mut buffer),
            Ok("music")
        );

        // exact buffer size
        assert_eq!(
            gbl_image_info_from_bytes(&[0x006d, 0x0075, 0x0073, 0x0069, 0x0063])
                .get_type_str(&mut buffer[..5]),
            Ok("music")
        );
        assert_eq!(
            gbl_image_info_from_bytes(&[0x006d, 0x0075, 0x0000, 0x0073, 0x0069, 0x0063])
                .get_type_str(&mut buffer[..2]),
            Ok("mu")
        );
    }

    #[test]
    fn test_image_info_get_type_str_small_buffer() {
        let mut buffer = [0u8; 8];
        assert_eq!(gbl_image_info_from_bytes(&UCS2_STR).get_type_str(&mut buffer), Err(10usize));
    }

    fn generate_protocol<'a, P: ProtocolInfo>(
        efi_entry: &'a EfiEntry,
        proto: &'a mut P::InterfaceType,
    ) -> Protocol<'a, P> {
        // SAFETY:
        // proto is a valid pointer and lasts at least as long as efi_entry.
        unsafe {
            Protocol::<'a, P>::new(
                DeviceHandle::new(null_mut()),
                NonNull::new(from_mut(proto)).unwrap(),
                efi_entry,
            )
        }
    }

    // Mutex to make sure tests that use `static RETURNED_BUFFERS` do not run in parallel to avoid
    // unexpected results since this is global static that would be shared between tests. And can
    // overflow due to amount of tests.
    //
    // See MEMORY_TEST thread local variable that should be used for convenience.
    static GET_BUFFER_MUTEX: Mutex<()> = Mutex::new(());

    // Size of MEMORY_TEST buffers
    const MEMORY_TEST_BUF_SIZE: usize = 100;

    // Helper struct for safe acquisition of the memory and releasing it on exit
    struct MemoryTest<'a> {
        // Tracking if test guard was acquired with `start()`
        init: bool,
        // Keep track of all buffers returned
        returned_buffers: HashSet<*mut [u8; MEMORY_TEST_BUF_SIZE]>,
        // Store same buffer value for `get_memory_same()` calls.
        same_buffer: Option<*mut c_void>,
        // It is necessary to run 1 test at a time that uses UEFI `get_buffer()`.
        // Because it is uses static size array to track returned values to prevent reusing same
        // buffer. With current number of test if they run simultaneously there are situations when
        // array limit is reached and unlucky test will fail. To prevent this flakiness this guard
        // is used.
        _get_buffer_guard: MutexGuard<'a, ()>,
    }

    thread_local! {
        static MEMORY_TEST: RefCell<MemoryTest<'static>> = RefCell::new(MemoryTest::new());
    }
    struct MemoryTestInitGuard {}

    impl Drop for MemoryTestInitGuard {
        fn drop(&mut self) {
            MEMORY_TEST.with_borrow_mut(|v| v.stop());
        }
    }

    // Helper implementation for getting raw buffers for `get_buffer()` calls.
    // And cleanly releasing buffers at the end of the test to prevent memory leaks.
    //
    // Use `thread_local` static MEMORY_TEST variable.
    //
    // ```
    // let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
    // ...
    // buffer.Memory = MEMORY_TEST.with_borrow_mut(|v| v.get_memory());
    // ...
    // ```
    // _memory_guard will make sure to cleanup all memory that was retrieved by `get_memory()` call
    //
    // Note:
    // If using raw EfiImageBuffer, there is no need for this helper. Since the structure does
    // cleaning on its own.
    // Except when using `EfiImageBuffer::take()` then manual `EfiImageBuffer::release()` must be
    // used.
    impl MemoryTest<'_> {
        fn new() -> Self {
            MemoryTest {
                init: false,
                returned_buffers: HashSet::new(),
                same_buffer: None,
                _get_buffer_guard: GET_BUFFER_MUTEX.lock(),
            }
        }

        fn start(&mut self) -> MemoryTestInitGuard {
            assert!(!self.init);
            self.init = true;
            MemoryTestInitGuard {}
        }

        // Return heap allocated buffer, and keep track of its address
        // To verify it was properly released
        //
        // # Safety
        //
        // Returned pointers must not be used after guard returned by `start()`
        // is destroyed.
        unsafe fn get_memory(&mut self) -> *mut c_void {
            assert!(self.init);
            let ptr = Box::into_raw(Box::new([0u8; MEMORY_TEST_BUF_SIZE]));
            assert!(self.returned_buffers.insert(ptr));
            ptr as *mut c_void
        }

        // Return same buffer for all calls, allocating and tracking it only for first call.
        //
        // # Safety
        //
        // Returned pointers must not be used after guard returned by `start()`
        // is destroyed.
        unsafe fn get_memory_same(&mut self) -> *mut c_void {
            if self.same_buffer.is_none() {
                // SAFETY:
                // This function has same requirements as `get_memory()`
                let address = unsafe { self.get_memory() };

                self.same_buffer = Some(address);
            }

            *self.same_buffer.as_mut().unwrap()
        }

        // Clear address from buffers returned list
        // Which allows to reuse it in other tests.
        fn stop(&mut self) {
            assert!(self.init);
            self.init = false;
            self.same_buffer = None;
            for ptr in self.returned_buffers.drain() {
                // SAFETY:
                // `ptr` is valid since was created by `Box::into_raw()`.
                // Double free is covered by safety requirements for this function. (`release_memory()`
                // must be called only on buffer holding the only reference to buffer.)
                // As well as tracking `returned_buffers` and asserting remove in the line above.
                unsafe {
                    let _restore_box = Box::from_raw(ptr);
                }
            }
        }
    }

    #[test]
    fn test_proto_get_buffer_error() {
        unsafe extern "efiapi" fn get_buffer(
            _: *mut GblEfiImageLoadingProtocol,
            _: *const GblEfiImageInfo,
            _: *mut GblEfiImageBuffer,
        ) -> EfiStatus {
            EFI_STATUS_INVALID_PARAMETER
        }

        run_test(|image_handle, systab_ptr| {
            let gbl_image_info: GblEfiImageInfo = Default::default();
            let mut image_loading =
                GblEfiImageLoadingProtocol { get_buffer: Some(get_buffer), ..Default::default() };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol =
                generate_protocol::<GblImageLoadingProtocol>(&efi_entry, &mut image_loading);

            let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
            assert!(protocol.get_buffer(&gbl_image_info).is_err());
        });
    }

    #[test]
    fn test_proto_get_buffer_return_alloc_size() {
        // SAFETY:
        // * Caler must guarantee that `buffer` points to a valid instance of `GblEfiImageBuffer`.
        unsafe extern "efiapi" fn get_buffer(
            _: *mut GblEfiImageLoadingProtocol,
            _: *const GblEfiImageInfo,
            buffer: *mut GblEfiImageBuffer,
        ) -> EfiStatus {
            // SAFETY
            // By safety requirement of this function, `buffer` points to a valid instance of
            // `GblEfiImageBuffer`.
            let buffer = unsafe { buffer.as_mut() }.unwrap();
            buffer.Memory = null_mut();
            buffer.SizeBytes = MEMORY_TEST_BUF_SIZE;
            EFI_STATUS_SUCCESS
        }

        run_test(|image_handle, systab_ptr| {
            let gbl_image_info: GblEfiImageInfo =
                GblEfiImageInfo { ImageType: [0; PARTITION_NAME_LEN_U16], SizeBytes: 100 };
            let mut image_loading =
                GblEfiImageLoadingProtocol { get_buffer: Some(get_buffer), ..Default::default() };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol =
                generate_protocol::<GblImageLoadingProtocol>(&efi_entry, &mut image_loading);
            let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
            assert!(matches!(
                protocol.get_buffer(&gbl_image_info),
                Ok(EfiImageBufferInfo::AllocSize(MEMORY_TEST_BUF_SIZE))
            ));
        });
    }

    #[test]
    fn test_proto_get_buffer_zero_size() {
        unsafe extern "efiapi" fn get_buffer(
            _: *mut GblEfiImageLoadingProtocol,
            image_info: *const GblEfiImageInfo,
            buffer: *mut GblEfiImageBuffer,
        ) -> EfiStatus {
            assert!(!image_info.is_null());
            assert!(!buffer.is_null());
            // SAFETY
            // `buffer` must be valid pointer to `GblEfiImageBuffer`
            let buffer = unsafe { buffer.as_mut() }.unwrap();

            // SAFETY:
            // `get_memory()` results are returned in `buffer` in `get_buffer()` function.
            // All usage of `get_buffer()` results are not leaving `run_test()` scope.
            // Same function where `start()` guard is acquired, so it will not outlive guard.
            unsafe {
                buffer.Memory = MEMORY_TEST.with_borrow_mut(|v| v.get_memory());
            }
            buffer.SizeBytes = 0;

            EFI_STATUS_SUCCESS
        }

        run_test(|image_handle, systab_ptr| {
            let gbl_image_info: GblEfiImageInfo = Default::default();
            let mut image_loading =
                GblEfiImageLoadingProtocol { get_buffer: Some(get_buffer), ..Default::default() };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol =
                generate_protocol::<GblImageLoadingProtocol>(&efi_entry, &mut image_loading);

            let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
            let mut res = protocol.get_buffer(&gbl_image_info).unwrap();
            assert!(res.buffer().as_ref().unwrap().is_empty());
        });
    }

    #[test]
    fn test_proto_get_buffer_small() {
        unsafe extern "efiapi" fn get_buffer(
            _: *mut GblEfiImageLoadingProtocol,
            image_info: *const GblEfiImageInfo,
            buffer: *mut GblEfiImageBuffer,
        ) -> EfiStatus {
            assert!(!image_info.is_null());
            // SAFETY
            // `image_info` must be valid pointer to `GblEfiImageInfo`
            let image_info = unsafe { image_info.as_ref() }.unwrap();
            assert!(!buffer.is_null());
            // SAFETY
            // `buffer` must be valid pointer to `GblEfiImageBuffer`
            let buffer = unsafe { buffer.as_mut() }.unwrap();

            // SAFETY:
            // `get_memory()` results are returned in `buffer` in `get_buffer()` function.
            // All usage of `get_buffer()` results are not leaving `run_test()` scope.
            // Same function where `start()` guard is acquired, so it will not outlive guard.
            unsafe {
                buffer.Memory = MEMORY_TEST.with_borrow_mut(|v| v.get_memory());
            }
            buffer.SizeBytes = image_info.SizeBytes - 1;

            EFI_STATUS_SUCCESS
        }

        run_test(|image_handle, systab_ptr| {
            let gbl_image_info: GblEfiImageInfo =
                GblEfiImageInfo { ImageType: [0; PARTITION_NAME_LEN_U16], SizeBytes: 10 };
            let mut image_loading =
                GblEfiImageLoadingProtocol { get_buffer: Some(get_buffer), ..Default::default() };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol =
                generate_protocol::<GblImageLoadingProtocol>(&efi_entry, &mut image_loading);

            let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
            let res = protocol.get_buffer(&gbl_image_info);
            assert_eq!(res.unwrap_err(), Error::BufferTooSmall(Some(10)));
        });
    }

    #[test]
    fn test_proto_get_buffer() {
        unsafe extern "efiapi" fn get_buffer(
            _: *mut GblEfiImageLoadingProtocol,
            image_info: *const GblEfiImageInfo,
            buffer: *mut GblEfiImageBuffer,
        ) -> EfiStatus {
            assert!(!image_info.is_null());
            assert!(!buffer.is_null());
            // SAFETY
            // `buffer` must be valid pointer to `GblEfiImageBuffer`
            let buffer = unsafe { buffer.as_mut() }.unwrap();

            // SAFETY:
            // `get_memory()` results are returned in `buffer` in `get_buffer()` function.
            // All usage of `get_buffer()` results are not leaving `run_test()` scope.
            // Same function where `start()` guard is acquired, so it will not outlive guard.
            unsafe {
                buffer.Memory = MEMORY_TEST.with_borrow_mut(|v| v.get_memory());
            }
            buffer.SizeBytes = MEMORY_TEST_BUF_SIZE;

            EFI_STATUS_SUCCESS
        }

        run_test(|image_handle, systab_ptr| {
            let gbl_image_info: GblEfiImageInfo =
                GblEfiImageInfo { ImageType: [0; PARTITION_NAME_LEN_U16], SizeBytes: 100 };
            let mut image_loading =
                GblEfiImageLoadingProtocol { get_buffer: Some(get_buffer), ..Default::default() };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol =
                generate_protocol::<GblImageLoadingProtocol>(&efi_entry, &mut image_loading);

            let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
            let mut buf = protocol.get_buffer(&gbl_image_info).unwrap();
            assert_ne!(buf.buffer().as_ref().unwrap().as_ptr(), null_mut());
            assert_eq!(buf.buffer().as_ref().unwrap().len(), 100);
        });
    }

    #[test]
    fn test_proto_get_buffer_image_type() {
        const IMAGE_TYPE_STR: &'static str = "test";
        unsafe extern "efiapi" fn get_buffer(
            _: *mut GblEfiImageLoadingProtocol,
            image_info: *const GblEfiImageInfo,
            buffer: *mut GblEfiImageBuffer,
        ) -> EfiStatus {
            assert!(!image_info.is_null());
            // SAFETY
            // `image_info` must be valid pointer to `GblEfiImageInfo`
            let image_info = unsafe { image_info.as_ref() }.unwrap();
            assert!(!buffer.is_null());
            // SAFETY
            // `buffer` must be valid pointer to `GblEfiImageBuffer`
            let buffer = unsafe { buffer.as_mut() }.unwrap();

            let mut buffer_utf8 = [0u8; 100];
            assert_eq!(image_info.get_type_str(&mut buffer_utf8).unwrap(), IMAGE_TYPE_STR);

            // SAFETY:
            // `get_memory()` results are returned in `buffer` in `get_buffer()` function.
            // All usage of `get_buffer()` results are not leaving `run_test()` scope.
            // Same function where `start()` guard is acquired, so it will not outlive guard.
            unsafe {
                buffer.Memory = MEMORY_TEST.with_borrow_mut(|v| v.get_memory());
            }
            buffer.SizeBytes = MEMORY_TEST_BUF_SIZE;

            EFI_STATUS_SUCCESS
        }

        run_test(|image_handle, systab_ptr| {
            let mut image_type = [0u16; PARTITION_NAME_LEN_U16];
            image_type[..4].copy_from_slice(&IMAGE_TYPE_STR.encode_utf16().collect::<Vec<u16>>());
            let gbl_image_info: GblEfiImageInfo =
                GblEfiImageInfo { ImageType: image_type, SizeBytes: 100 };
            let mut image_loading =
                GblEfiImageLoadingProtocol { get_buffer: Some(get_buffer), ..Default::default() };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol =
                generate_protocol::<GblImageLoadingProtocol>(&efi_entry, &mut image_loading);

            let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
            assert!(protocol.get_buffer(&gbl_image_info).is_ok());
        });
    }

    #[test]
    fn test_proto_get_buffer_double_call() {
        unsafe extern "efiapi" fn get_buffer(
            _: *mut GblEfiImageLoadingProtocol,
            image_info: *const GblEfiImageInfo,
            buffer: *mut GblEfiImageBuffer,
        ) -> EfiStatus {
            assert!(!image_info.is_null());
            assert!(!buffer.is_null());
            // SAFETY
            // `buffer` must be valid pointer to `GblEfiImageBuffer`
            let buffer = unsafe { buffer.as_mut() }.unwrap();

            // SAFETY:
            // `get_memory_same()` results are returned in `buffer` in `get_buffer()` function.
            // All usage of `get_buffer()` results are not leaving `run_test()` scope.
            // Same function where `start()` guard is acquired, so it will not outlive guard.
            unsafe {
                buffer.Memory = MEMORY_TEST.with_borrow_mut(|v| v.get_memory_same());
            }
            buffer.SizeBytes = MEMORY_TEST_BUF_SIZE;

            EFI_STATUS_SUCCESS
        }

        run_test(|image_handle, systab_ptr| {
            let gbl_image_info: GblEfiImageInfo =
                GblEfiImageInfo { ImageType: [0; PARTITION_NAME_LEN_U16], SizeBytes: 100 };
            let mut image_loading =
                GblEfiImageLoadingProtocol { get_buffer: Some(get_buffer), ..Default::default() };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol =
                generate_protocol::<GblImageLoadingProtocol>(&efi_entry, &mut image_loading);

            let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
            let _buf = protocol.get_buffer(&gbl_image_info).unwrap();
            assert_eq!(protocol.get_buffer(&gbl_image_info).unwrap_err(), Error::AlreadyStarted);
        });
    }

    #[test]
    fn test_proto_get_buffer_double_call_after_drop() {
        unsafe extern "efiapi" fn get_buffer(
            _: *mut GblEfiImageLoadingProtocol,
            image_info: *const GblEfiImageInfo,
            buffer: *mut GblEfiImageBuffer,
        ) -> EfiStatus {
            assert!(!image_info.is_null());
            assert!(!buffer.is_null());
            // SAFETY
            // `buffer` must be valid pointer to `GblEfiImageBuffer`
            let buffer = unsafe { buffer.as_mut() }.unwrap();

            // SAFETY:
            // `get_memory()` results are returned in `buffer` in `get_buffer()` function.
            // All usage of `get_buffer()` results are not leaving `run_test()` scope.
            // Same function where `start()` guard is acquired, so it will not outlive guard.
            unsafe {
                buffer.Memory = MEMORY_TEST.with_borrow_mut(|v| v.get_memory_same());
            }
            buffer.SizeBytes = MEMORY_TEST_BUF_SIZE;

            EFI_STATUS_SUCCESS
        }

        run_test(|image_handle, systab_ptr| {
            let gbl_image_info: GblEfiImageInfo =
                GblEfiImageInfo { ImageType: [0; PARTITION_NAME_LEN_U16], SizeBytes: 100 };
            let mut image_loading =
                GblEfiImageLoadingProtocol { get_buffer: Some(get_buffer), ..Default::default() };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol =
                generate_protocol::<GblImageLoadingProtocol>(&efi_entry, &mut image_loading);

            let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
            protocol.get_buffer(&gbl_image_info).unwrap();
            protocol.get_buffer(&gbl_image_info).unwrap();
        });
    }

    #[test]
    #[should_panic]
    fn test_proto_get_buffer_too_many_times() {
        unsafe extern "efiapi" fn get_buffer(
            _: *mut GblEfiImageLoadingProtocol,
            image_info: *const GblEfiImageInfo,
            buffer: *mut GblEfiImageBuffer,
        ) -> EfiStatus {
            assert!(!image_info.is_null());
            assert!(!buffer.is_null());
            // SAFETY
            // `buffer` must be valid pointer to `GblEfiImageBuffer`
            let buffer = unsafe { buffer.as_mut() }.unwrap();

            // SAFETY:
            // `get_memory()` results are returned in `buffer` in `get_buffer()` function.
            // All usage of `get_buffer()` results are not leaving `run_test()` scope.
            // Same function where `start()` guard is acquired, so it will not outlive guard.
            unsafe {
                buffer.Memory = MEMORY_TEST.with_borrow_mut(|v| v.get_memory());
            }
            buffer.SizeBytes = MEMORY_TEST_BUF_SIZE;

            EFI_STATUS_SUCCESS
        }

        run_test(|image_handle, systab_ptr| {
            let gbl_image_info: GblEfiImageInfo =
                GblEfiImageInfo { ImageType: [0; PARTITION_NAME_LEN_U16], SizeBytes: 100 };
            let mut image_loading =
                GblEfiImageLoadingProtocol { get_buffer: Some(get_buffer), ..Default::default() };
            let efi_entry = EfiEntry { image_handle, systab_ptr };
            let protocol =
                generate_protocol::<GblImageLoadingProtocol>(&efi_entry, &mut image_loading);

            let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
            let mut keep_alive: Vec<EfiImageBufferInfo> = vec![];
            for _ in 1..=MAX_ARRAY_SIZE + 1 {
                keep_alive.push(protocol.get_buffer(&gbl_image_info).unwrap());
            }
        });
    }

    #[test]
    fn test_efi_image_buffer() {
        let mut v = vec![0u8; 1];
        let gbl_buffer =
            GblEfiImageBuffer { Memory: v.as_mut_ptr() as *mut c_void, SizeBytes: v.len() };

        let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
        // SAFETY:
        // 'gbl_buffer` represents valid buffer created by vector.
        let res = unsafe { EfiImageBuffer::new(gbl_buffer) };
        assert!(res.is_ok());
    }

    #[test]
    fn test_efi_image_buffer_null() {
        let gbl_buffer = GblEfiImageBuffer { Memory: null_mut(), SizeBytes: 1 };

        let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
        // SAFETY:
        // 'gbl_buffer` contains Memory == NULL, which is valid input value. And we expect Error as
        // a result
        let res = unsafe { EfiImageBuffer::new(gbl_buffer) };
        assert_eq!(res.unwrap_err(), Error::InvalidInput);
    }

    #[test]
    fn test_efi_image_buffer_same_buffer() {
        let mut v = vec![0u8; 1];
        let gbl_buffer =
            GblEfiImageBuffer { Memory: v.as_mut_ptr() as *mut c_void, SizeBytes: v.len() };

        let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
        // SAFETY:
        // 'gbl_buffer` represents valid buffer created by vector.
        let res1 = unsafe { EfiImageBuffer::new(gbl_buffer) };
        assert!(res1.is_ok());

        // Since we keep `res1`, second return of same buffer should fail
        // SAFETY:
        // 'gbl_buffer` represents valid buffer created by vector.
        let res2 = unsafe { EfiImageBuffer::new(gbl_buffer) };
        assert_eq!(res2.unwrap_err(), Error::AlreadyStarted);
    }

    #[test]
    fn test_efi_image_buffer_same_buffer_after_drop() {
        let mut v = vec![0u8; 1];
        let gbl_buffer =
            GblEfiImageBuffer { Memory: v.as_mut_ptr() as *mut c_void, SizeBytes: v.len() };

        let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
        // SAFETY:
        // 'gbl_buffer` represents valid buffer created by vector.
        let res1 = unsafe { EfiImageBuffer::new(gbl_buffer) };
        drop(res1);

        // Since `res1` was dropped same buffer can be returned.
        // SAFETY:
        // 'gbl_buffer` represents valid buffer created by vector.
        let res2 = unsafe { EfiImageBuffer::new(gbl_buffer) };
        assert!(res2.is_ok());
    }

    #[test]
    fn test_efi_image_buffer_take() {
        let mut v = vec![0u8; 1];
        let gbl_buffer =
            GblEfiImageBuffer { Memory: v.as_mut_ptr() as *mut c_void, SizeBytes: v.len() };

        let _memory_guard = MEMORY_TEST.with_borrow_mut(|v| v.start());
        // SAFETY:
        // 'gbl_buffer` represents valid buffer created by vector.
        let mut res1 = unsafe { EfiImageBuffer::new(gbl_buffer) }.unwrap();
        let buf_no_owner = res1.take();

        // Since `res1` was taken, we can't reuse same buffer.
        // SAFETY:
        // 'gbl_buffer` represents valid buffer created by vector.
        let res2 = unsafe { EfiImageBuffer::new(gbl_buffer) };
        assert_eq!(res2.unwrap_err(), Error::AlreadyStarted);

        // Make sure to clean tracking
        // SAFETY:
        // `buf_no_owner` is the only reference to buffer
        unsafe {
            EfiImageBuffer::release(buf_no_owner.as_ptr() as usize);
        }
    }
}
