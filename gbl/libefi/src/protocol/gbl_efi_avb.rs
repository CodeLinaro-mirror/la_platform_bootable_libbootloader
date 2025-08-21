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

//! Rust wrapper for `GBL_EFI_AVB_PROTOCOL`.

use crate::efi_call;
use crate::protocol::{Protocol, ProtocolInfo};
use core::ffi::CStr;
use core::ptr::null;
use efi_types::{
    EfiGuid, GblEfiAvbKeyValidationStatus, GblEfiAvbPartition, GblEfiAvbProtocol,
    GblEfiAvbVerificationResult,
};
use liberror::Result;

/// `GBL_EFI_AVB_PROTOCOL` implementation.
pub struct GblAvbProtocol;

impl ProtocolInfo for GblAvbProtocol {
    type InterfaceType = GblEfiAvbProtocol;

    const GUID: EfiGuid =
        EfiGuid::new(0x6bc66b9a, 0xd5c9, 0x4c02, [0x9d, 0xa9, 0x50, 0xaf, 0x19, 0x8d, 0x91, 0x2c]);
}

// Protocol interface wrappers.
impl Protocol<'_, GblAvbProtocol> {
    /// Wrapper of `GBL_EFI_AVB_PROTOCOL.read_partitions_to_verify()`.
    ///
    /// # Result
    /// Err(BufferTooSmall(Some(size))) - when provided `partitions` is less than expected `size`.
    /// Err(err) - if error occurred.
    /// Ok(len) - will return number of `GblEfiAvbPartition`s copied to `partitions` slice.
    ///
    /// SAFETY:
    /// * Each `partitions[N].base_name` must point to non-null writable buffer of at least
    /// `partitions[N].base_name_len` bytes.
    pub unsafe fn read_partitions_to_verify(
        &self,
        partitions: &mut [GblEfiAvbPartition],
    ) -> Result<usize> {
        let mut num_partitions = partitions.len();

        // SAFETY:
        // * `self.interface_ptr()` points to a valid object established by `Protocol::new()`.
        // * `self.interface_ptr()` is input parameter, outlives the call, and will not be retained.
        // * `num_partitions` is input/output parameter, non-null and points to a valid writtable
        //   usize buffer.
        // * `partitions` is input/output parameter, non-null and points to `partitions.len()`
        //   consecutive `GblEfiAvbPartition`. Each `partitions[N].base_name` points to writable
        //   buffer of at least `partitions[N].base_name_len` bytes.
        unsafe {
            efi_call!(
                @bufsize num_partitions,
                self.interface().read_partitions_to_verify,
                self.interface_ptr(),
                &mut num_partitions,
                partitions.as_mut_ptr(),
            )?;
        }

        Ok(num_partitions)
    }

    /// Wraps `GBL_EFI_AVB_PROTOCOL.read_device_status()`.
    pub fn read_device_status(&self) -> Result<u64> {
        let mut flags: u64 = 0;

        // SAFETY:
        // * `self.interface_ptr()` points to a valid object established by `Protocol::new()`
        // * `flags` is non-null buffer points to a `u64` available to write, must be used
        //   only within the call
        unsafe { efi_call!(self.interface().read_device_status, self.interface_ptr(), &mut flags)? }

        Ok(flags)
    }

    /// Wraps `GBL_EFI_AVB_PROTOCOL.validate_vbmeta_public_key()`.
    pub fn validate_vbmeta_public_key(
        &self,
        public_key: &[u8],
        public_key_metadata: Option<&[u8]>,
    ) -> Result<GblEfiAvbKeyValidationStatus> {
        let mut validation_status =
            efi_types::GBL_EFI_AVB_KEY_VALIDATION_STATUS_GBL_EFI_AVB_KEY_INVALID as _;

        // SAFETY:
        // * `self.interface_ptr()` points to a valid object established by `Protocol::new()`
        // * `public_key` pointer is not-null and used only within the call
        // * `public_key_metadata` pointer (can be null), used only within the call
        // * `validation_status` non-null pointer available to write
        unsafe {
            efi_call!(
                self.interface().validate_vbmeta_public_key,
                self.interface_ptr(),
                public_key.len(),
                public_key.as_ptr() as *const _,
                public_key_metadata.map_or(0, |m| m.len()),
                public_key_metadata.map_or(null(), |m| m.as_ptr() as *const _),
                &mut validation_status,
            )?
        }

        Ok(validation_status as _)
    }

    /// Wraps `GBL_EFI_AVB_PROTOCOL.read_rollback_index()`.
    pub fn read_rollback_index(&self, index_location: usize) -> Result<u64> {
        let mut rollback_index: u64 = 0;

        // SAFETY:
        // * `self.interface_ptr()` guarantees `self.interface_ptr()` is non-null and points to a valid
        //   object established by `Protocol::new()`.
        // * `rollback_index` is a valid pointer to a `u64` available for write.
        unsafe {
            efi_call!(
                self.interface().read_rollback_index,
                self.interface_ptr(),
                index_location,
                &mut rollback_index,
            )?
        }

        Ok(rollback_index)
    }

    /// Wraps `GBL_EFI_AVB_PROTOCOL.write_rollback_index()`.
    pub fn write_rollback_index(&self, index_location: usize, rollback_index: u64) -> Result<()> {
        // SAFETY:
        // * `self.interface_ptr()` guarantees `self.interface_ptr()` is non-null and points to a valid
        //   object established by `Protocol::new()`.
        unsafe {
            efi_call!(
                self.interface().write_rollback_index,
                self.interface_ptr(),
                index_location,
                rollback_index,
            )?
        }

        Ok(())
    }

    /// Wraps `GBL_EFI_AVB_PROTOCOL.read_persistent_value()`.
    pub fn read_persistent_value(&self, name: &CStr, value: &mut [u8]) -> Result<usize> {
        let mut value_buffer_size = value.len();

        let value_ptr = match value.is_empty() {
            true => core::ptr::null_mut(),
            false => value.as_mut_ptr(),
        };

        // SAFETY:
        // * `self.interface_ptr()` guarantees `self.interface_ptr()` is non-null and points to a valid
        //   object established by `Protocol::new()`.
        // * `name` is a valid pointer to a null-terminated string used only within the call.
        // * `value_buffer_size` holds a mutable reference to `usize`, used only within the call.
        // * `value_ptr` is either a valid pointer to a writable buffer or a null pointer, used only
        //   within the call
        unsafe {
            efi_call!(
                @bufsize value_buffer_size,
                self.interface().read_persistent_value,
                self.interface_ptr(),
                name.as_ptr() as _,
                &mut value_buffer_size,
                value_ptr,
            )?
        }

        Ok(value_buffer_size)
    }

    /// Wraps `GBL_EFI_AVB_PROTOCOL.write_persistent_value()`.
    pub fn write_persistent_value(&self, name: &CStr, value: Option<&[u8]>) -> Result<()> {
        let (value_ptr, value_len) = match value {
            Some(v) => (v.as_ptr(), v.len()),
            None => (core::ptr::null(), 0),
        };

        // SAFETY:
        // * `self.interface_ptr()` guarantees `self.interface_ptr()` is non-null and points to a valid
        //   object established by `Protocol::new()`.
        // * `name` is a valid pointer to a null-terminated string used only within the call.
        // * `value_ptr` is a valid pointer to `value_len` sized buffer or null, used only within
        //   the call.
        unsafe {
            efi_call!(
                self.interface().write_persistent_value,
                self.interface_ptr(),
                name.as_ptr() as _,
                value_len,
                value_ptr,
            )?
        }

        Ok(())
    }

    /// Wraps `GBL_EFI_AVB_PROTOCOL.handle_verification_result()`.
    pub fn handle_verification_result(
        &self,
        verification_result: &GblEfiAvbVerificationResult,
    ) -> Result<()> {
        // SAFETY:
        // * `self.interface_ptr()` points to a valid object established by `Protocol::new()`.
        // * `verification_result` pointer is not-null and used only within the call.
        unsafe {
            efi_call!(
                self.interface().handle_verification_result,
                self.interface_ptr(),
                verification_result as *const _
            )
        }
    }

    /// Wraps `GBL_EFI_AVB_PROTOCOL.revision`.
    pub fn revision(&self) -> u64 {
        self.interface().revision
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{test::run_test_with_mock_protocol, Error};
    use efi_types::defs::{
        EfiStatus, EFI_STATUS_BUFFER_TOO_SMALL, EFI_STATUS_INVALID_PARAMETER, EFI_STATUS_SUCCESS,
    };
    use std::{ptr, slice};

    #[test]
    fn read_partitions_to_verify_partitions_provided() {
        const PARTITIONS_NUM: usize = 3;
        const PARTITION_MAX_LEN: usize = 29;
        const FIRST_PROVIDED_PARTITION: &[u8] = b"first_partition";
        const SECOND_PROVIDED_PARTITION: &[u8] = b"second_partition";

        const EXPECTED_PROVIDED_PARTITIONS_NUM: usize = 2;

        /// C callback implementation that provides EXPECTED_PROVIDED_PARTITIONS_NUM partitions.
        unsafe extern "efiapi" fn read_partitions_to_verify(
            _: *mut GblEfiAvbProtocol,
            num_partitions: *mut usize,
            partitions: *mut GblEfiAvbPartition,
        ) -> EfiStatus {
            // SAFETY:
            // * `num_partitions` points to non-null writtable usize buffer.
            // * `partitions` points to writtable buffer with `num_partitions` amount of
            //   `GblEfiAvbPartition`.
            // * Each `partitions[N].base_name` points to writable buffer of at least
            //   `partitions[N].base_name_len` bytes.
            unsafe {
                let partitions = core::slice::from_raw_parts_mut(partitions, *num_partitions);
                let first = core::slice::from_raw_parts_mut(
                    partitions[0].base_name,
                    partitions[0].base_name_len,
                );
                let second = core::slice::from_raw_parts_mut(
                    partitions[1].base_name,
                    partitions[1].base_name_len,
                );

                first[..FIRST_PROVIDED_PARTITION.len()].copy_from_slice(FIRST_PROVIDED_PARTITION);
                second[..SECOND_PROVIDED_PARTITION.len()]
                    .copy_from_slice(SECOND_PROVIDED_PARTITION);

                partitions[0].base_name_len = FIRST_PROVIDED_PARTITION.len();
                partitions[1].base_name_len = SECOND_PROVIDED_PARTITION.len();

                *num_partitions = EXPECTED_PROVIDED_PARTITIONS_NUM
            }

            EFI_STATUS_SUCCESS
        }

        let c_interface = GblEfiAvbProtocol {
            read_partitions_to_verify: Some(read_partitions_to_verify),
            ..Default::default()
        };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            let mut partitions = [GblEfiAvbPartition::default(); PARTITIONS_NUM];

            let first_name = &mut [0u8; PARTITION_MAX_LEN];
            let second_name = &mut [0u8; PARTITION_MAX_LEN];
            let third_name = &mut [0u8; PARTITION_MAX_LEN];

            partitions[0].base_name_len = PARTITION_MAX_LEN;
            partitions[0].base_name = first_name.as_mut_ptr();
            partitions[1].base_name_len = PARTITION_MAX_LEN;
            partitions[1].base_name = second_name.as_mut_ptr();
            partitions[2].base_name_len = PARTITION_MAX_LEN;
            partitions[2].base_name = third_name.as_mut_ptr();

            // SAFETY:
            // * Each `partitions[N].base_name` points to writable buffer of at least
            // `partitions[N].base_name_len` bytes.
            let result = unsafe { avb_protocol.read_partitions_to_verify(&mut partitions) };
            assert_eq!(result, Ok(EXPECTED_PROVIDED_PARTITIONS_NUM));

            // SAFETY:
            // * Each `partitions[N].base_name` points to writable buffer of at least
            // `partitions[N].base_name_len` bytes.
            let (first_name, second_name) = unsafe {
                (
                    core::slice::from_raw_parts(
                        partitions[0].base_name,
                        partitions[0].base_name_len,
                    ),
                    core::slice::from_raw_parts(
                        partitions[1].base_name,
                        partitions[1].base_name_len,
                    ),
                )
            };
            assert_eq!(
                [first_name, second_name],
                [FIRST_PROVIDED_PARTITION, SECOND_PROVIDED_PARTITION],
            );
        });
    }

    #[test]
    fn read_partitions_to_verify_buffer_too_small() {
        const EXPECTED_PARTITIONS_NUM: usize = 2;

        /// C callback implementation that requests a larger buffer.
        unsafe extern "efiapi" fn read_partitions_to_verify(
            _: *mut GblEfiAvbProtocol,
            num_partitions: *mut usize,
            _: *mut GblEfiAvbPartition,
        ) -> EfiStatus {
            // SAFETY: `num_partitions` is non-null pointer to writable usize buffer.
            unsafe { *num_partitions = EXPECTED_PARTITIONS_NUM };
            EFI_STATUS_BUFFER_TOO_SMALL
        }

        let c_interface = GblEfiAvbProtocol {
            read_partitions_to_verify: Some(read_partitions_to_verify),
            ..Default::default()
        };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            let mut partitions: [GblEfiAvbPartition; 0] = [];

            // SAFETY:
            // * Each `partitions[N].base_name` points to writable buffer of at least
            // `partitions[N].base_name_len` bytes.
            let result = unsafe { avb_protocol.read_partitions_to_verify(&mut partitions) };
            assert_eq!(result, Err(Error::BufferTooSmall(Some(EXPECTED_PARTITIONS_NUM))));
        });
    }

    #[test]
    fn read_partitions_to_verify_error() {
        /// C callback implementation that returns an error.
        unsafe extern "efiapi" fn read_partitions_to_verify(
            _: *mut GblEfiAvbProtocol,
            _: *mut usize,
            _: *mut GblEfiAvbPartition,
        ) -> EfiStatus {
            EFI_STATUS_INVALID_PARAMETER
        }

        let c_interface = GblEfiAvbProtocol {
            read_partitions_to_verify: Some(read_partitions_to_verify),
            ..Default::default()
        };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            let mut partitions: [GblEfiAvbPartition; 0] = [];

            // SAFETY:
            // * Each `partitions[N].base_name` points to writable buffer of at least
            // `partitions[N].base_name_len` bytes.
            let result = unsafe { avb_protocol.read_partitions_to_verify(&mut partitions) };
            assert_eq!(result, Err(Error::InvalidInput));
        });
    }

    #[test]
    fn read_device_status_returns_unlocked() {
        /// C callback implementation that sets the flags for unlocked status.
        unsafe extern "efiapi" fn c_return_unlocked_and_ok(
            _: *mut GblEfiAvbProtocol,
            flags_ptr: *mut u64,
        ) -> EfiStatus {
            // SAFETY: flags_ptr is a valid u64 pointer available to write.
            unsafe {
                *flags_ptr = efi_types::GBL_EFI_AVB_DEVICE_STATUS_GBL_EFI_AVB_STATUS_UNLOCKED as u64
            };
            EFI_STATUS_SUCCESS
        }

        let c_interface = GblEfiAvbProtocol {
            read_device_status: Some(c_return_unlocked_and_ok),
            ..Default::default()
        };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            let expected_flags =
                efi_types::GBL_EFI_AVB_DEVICE_STATUS_GBL_EFI_AVB_STATUS_UNLOCKED as u64;
            assert_eq!(avb_protocol.read_device_status(), Ok(expected_flags));
        });
    }

    #[test]
    fn read_device_status_error_handled() {
        /// C callback implementation that returns an error.
        unsafe extern "efiapi" fn c_return_error(
            _: *mut GblEfiAvbProtocol,
            _: *mut u64,
        ) -> EfiStatus {
            EFI_STATUS_INVALID_PARAMETER
        }

        let c_interface =
            GblEfiAvbProtocol { read_device_status: Some(c_return_error), ..Default::default() };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            assert_eq!(avb_protocol.read_device_status(), Err(Error::InvalidInput));
        });
    }

    #[test]
    fn validate_vbmeta_public_key_status_provided() {
        const EXPECTED_PUBLIC_KEY: &[u8] = b"test_key";
        const EXPECTED_STATUS: GblEfiAvbKeyValidationStatus =
            efi_types::GBL_EFI_AVB_KEY_VALIDATION_STATUS_GBL_EFI_AVB_KEY_VALID_CUSTOM_KEY;

        // C callback implementation that returns an error
        unsafe extern "efiapi" fn c_return_error(
            _: *mut GblEfiAvbProtocol,
            public_key_len: usize,
            public_key_ptr: *const u8,
            _metadata_len: usize,
            _metadata_ptr: *const u8,
            validation_status_ptr: *mut GblEfiAvbKeyValidationStatus,
        ) -> EfiStatus {
            // SAFETY:
            // * `public_key_ptr` is a non-null pointer to the buffer at least `public_key_len`
            // size.
            let public_key_buffer =
                unsafe { slice::from_raw_parts(public_key_ptr, public_key_len) };

            assert_eq!(public_key_buffer, EXPECTED_PUBLIC_KEY);

            // SAFETY:
            // * `validation_status_ptr` is a non-null pointer to GblEfiAvbKeyValidationStatus
            // available to write.
            unsafe { *validation_status_ptr = EXPECTED_STATUS };

            EFI_STATUS_SUCCESS
        }

        let c_interface = GblEfiAvbProtocol {
            validate_vbmeta_public_key: Some(c_return_error),
            ..Default::default()
        };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            assert_eq!(
                avb_protocol.validate_vbmeta_public_key(EXPECTED_PUBLIC_KEY, None),
                Ok(EXPECTED_STATUS)
            );
        });
    }

    #[test]
    fn validate_vbmeta_public_key_error_handled() {
        // C callback implementation that returns an error
        unsafe extern "efiapi" fn c_return_error(
            _: *mut GblEfiAvbProtocol,
            _public_key_len: usize,
            _public_key_ptr: *const u8,
            _metadata_len: usize,
            _metadata_ptr: *const u8,
            _validation_status_ptr: *mut GblEfiAvbKeyValidationStatus,
        ) -> EfiStatus {
            EFI_STATUS_INVALID_PARAMETER
        }

        let c_interface = GblEfiAvbProtocol {
            validate_vbmeta_public_key: Some(c_return_error),
            ..Default::default()
        };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            assert!(avb_protocol.validate_vbmeta_public_key(b"test_key", None).is_err());
        });
    }

    #[test]
    fn handle_verification_result_data_provided() {
        const COLOR: u32 = efi_types::GBL_EFI_AVB_BOOT_COLOR_GBL_EFI_AVB_COLOR_RED;

        // C callback implementation that returns success.
        unsafe extern "efiapi" fn c_return_success(
            _: *mut GblEfiAvbProtocol,
            result: *const GblEfiAvbVerificationResult,
        ) -> EfiStatus {
            // SAFETY:
            // * `result` is non-null.
            assert_eq!(unsafe { (*result).color }, COLOR);
            EFI_STATUS_SUCCESS
        }

        let c_interface = GblEfiAvbProtocol {
            handle_verification_result: Some(c_return_success),
            ..Default::default()
        };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            let verification_result =
                GblEfiAvbVerificationResult { color: COLOR, ..Default::default() };

            assert!(avb_protocol.handle_verification_result(&verification_result).is_ok());
        });
    }

    #[test]
    fn handle_verification_result_error() {
        // C callback implementation that returns an error.
        unsafe extern "efiapi" fn c_return_error(
            _: *mut GblEfiAvbProtocol,
            _: *const GblEfiAvbVerificationResult,
        ) -> EfiStatus {
            EFI_STATUS_INVALID_PARAMETER
        }

        let c_interface = GblEfiAvbProtocol {
            handle_verification_result: Some(c_return_error),
            ..Default::default()
        };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            let verification_result = GblEfiAvbVerificationResult::default();

            assert!(avb_protocol.handle_verification_result(&verification_result).is_err());
        });
    }

    #[test]
    fn read_rollback_index_returns_value() {
        const EXPECTED_INDEX_LOCATION: usize = 1;
        const EXPECTED_ROLLBACK_INDEX: u64 = 42;

        /// C callback implementation that sets rollback_index to EXPECTED_ROLLBACK_INDEX.
        ///
        /// # Safety:
        /// Caller must guaranteed that `rollback_index_ptr` points to a valid u64 variable
        /// available for write.
        unsafe extern "efiapi" fn c_return_value(
            _: *mut GblEfiAvbProtocol,
            index_location: usize,
            rollback_index_ptr: *mut u64,
        ) -> EfiStatus {
            assert_eq!(index_location, EXPECTED_INDEX_LOCATION);

            // SAFETY: By safety requirement of this function, `rollback_index_ptr` is a valid
            // pointer.
            unsafe { *rollback_index_ptr = EXPECTED_ROLLBACK_INDEX };
            EFI_STATUS_SUCCESS
        }

        let c_interface =
            GblEfiAvbProtocol { read_rollback_index: Some(c_return_value), ..Default::default() };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            assert_eq!(
                avb_protocol.read_rollback_index(EXPECTED_INDEX_LOCATION),
                Ok(EXPECTED_ROLLBACK_INDEX)
            );
        });
    }

    #[test]
    fn read_rollback_index_error_handled() {
        /// C callback implementation that returns an error.
        unsafe extern "efiapi" fn c_return_error(
            _: *mut GblEfiAvbProtocol,
            _: usize,
            _: *mut u64,
        ) -> EfiStatus {
            EFI_STATUS_INVALID_PARAMETER
        }

        let c_interface =
            GblEfiAvbProtocol { read_rollback_index: Some(c_return_error), ..Default::default() };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            assert!(avb_protocol.read_rollback_index(0).is_err());
        });
    }

    #[test]
    fn write_rollback_index_success() {
        const EXPECTED_INDEX_LOCATION: usize = 1;
        const EXPECTED_ROLLBACK_INDEX: u64 = 42;

        /// C callback implementation that checks the passed parameters and returns success.
        unsafe extern "efiapi" fn c_return_success(
            _: *mut GblEfiAvbProtocol,
            index_location: usize,
            rollback_index: u64,
        ) -> EfiStatus {
            assert_eq!(index_location, EXPECTED_INDEX_LOCATION);
            assert_eq!(rollback_index, EXPECTED_ROLLBACK_INDEX);
            EFI_STATUS_SUCCESS
        }

        let c_interface = GblEfiAvbProtocol {
            write_rollback_index: Some(c_return_success),
            ..Default::default()
        };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            assert!(avb_protocol
                .write_rollback_index(EXPECTED_INDEX_LOCATION, EXPECTED_ROLLBACK_INDEX)
                .is_ok());
        });
    }

    #[test]
    fn write_rollback_index_error_handled() {
        /// C callback implementation that returns an error.
        unsafe extern "efiapi" fn c_return_error(
            _: *mut GblEfiAvbProtocol,
            _: usize,
            _: u64,
        ) -> EfiStatus {
            EFI_STATUS_INVALID_PARAMETER
        }

        let c_interface =
            GblEfiAvbProtocol { write_rollback_index: Some(c_return_error), ..Default::default() };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            assert!(avb_protocol.write_rollback_index(0, 0).is_err());
        });
    }

    #[test]
    fn read_persistent_value_success() {
        const EXPECTED_NAME: &CStr = c"test_key";
        const EXPECTED_VALUE: &[u8] = b"test_value";

        /// C callback implementation.
        ///
        /// # Safety:
        /// * Caller must guaranteed that `name` points to a valid null-terminated string.
        /// * Caller must guaranteed that `value_size` points to a valid usize available to write
        ///   value buffer.
        /// * Caller must guaranteed that `value` points to non-null `value_size` sized bytes
        ///   buffer.
        unsafe extern "efiapi" fn c_read_persistent_value_success(
            _: *mut GblEfiAvbProtocol,
            name: *const u8,
            value_size: *mut usize,
            value: *mut u8,
        ) -> EfiStatus {
            assert_eq!(
                // SAFETY:
                // * `name` is a valid pointer to null-terminated string.
                unsafe { CStr::from_ptr(name as _) },
                EXPECTED_NAME
            );
            assert_eq!(
                // SAFETY:
                // * `value_size` is a valid non-null pointer to `usize` value.
                unsafe { ptr::read(value_size) },
                EXPECTED_VALUE.len()
            );

            // SAFETY:
            // * `value` is non-null pointer available for write.
            let value_buffer = unsafe { slice::from_raw_parts_mut(value, EXPECTED_VALUE.len()) };
            value_buffer.copy_from_slice(EXPECTED_VALUE);

            return EFI_STATUS_SUCCESS;
        }

        let c_interface = GblEfiAvbProtocol {
            read_persistent_value: Some(c_read_persistent_value_success),
            ..Default::default()
        };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            let mut buffer = [0u8; EXPECTED_VALUE.len()];

            assert_eq!(
                avb_protocol.read_persistent_value(EXPECTED_NAME, &mut buffer),
                Ok(EXPECTED_VALUE.len())
            );
            assert_eq!(&buffer, EXPECTED_VALUE);
        });
    }

    #[test]
    fn read_persistent_value_buffer_too_small() {
        const EXPECTED_BUFFER_SIZE: usize = 12;

        /// C callback implementation.
        ///
        /// # Safety:
        /// * Caller must guaranteed that `value_size` points to a valid usize available to write
        ///   value buffer.
        unsafe extern "efiapi" fn c_read_persistent_value_buffer_too_small(
            _: *mut GblEfiAvbProtocol,
            _: *const u8,
            value_size: *mut usize,
            _: *mut u8,
        ) -> EfiStatus {
            // SAFETY:
            // * `value_size` is a valid non-null pointer to `usize` value.
            unsafe { ptr::write(value_size, EXPECTED_BUFFER_SIZE) };

            return EFI_STATUS_BUFFER_TOO_SMALL;
        }

        let c_interface = GblEfiAvbProtocol {
            read_persistent_value: Some(c_read_persistent_value_buffer_too_small),
            ..Default::default()
        };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            let mut buffer = [0u8; 0];

            assert_eq!(
                avb_protocol.read_persistent_value(c"name", &mut buffer),
                Err(Error::BufferTooSmall(Some(EXPECTED_BUFFER_SIZE)))
            );
        });
    }

    #[test]
    fn write_persistent_value_success() {
        const EXPECTED_NAME: &CStr = c"test_key";
        const EXPECTED_VALUE: &[u8] = b"test_value";

        /// C callback implementation.
        ///
        /// # Safety:
        /// * Caller must guarantee that `name` points to a valid null-terminated string.
        /// * Caller must guarantee that `value` points to a valid `value_size` sized bytes buffer.
        unsafe extern "efiapi" fn c_write_persistent_value_success(
            _: *mut GblEfiAvbProtocol,
            name: *const u8,
            value_size: usize,
            value: *const u8,
        ) -> EfiStatus {
            assert_eq!(
                // SAFETY:
                // * `name` is a valid pointer to null-terminated string.
                unsafe { CStr::from_ptr(name as _) },
                EXPECTED_NAME
            );
            assert_eq!(value_size, EXPECTED_VALUE.len());

            // SAFETY:
            // * `value` is a valid pointer to `value_size` bytes.
            let value_buffer = unsafe { slice::from_raw_parts(value, value_size) };
            assert_eq!(value_buffer, EXPECTED_VALUE);

            return EFI_STATUS_SUCCESS;
        }

        let c_interface = GblEfiAvbProtocol {
            write_persistent_value: Some(c_write_persistent_value_success),
            ..Default::default()
        };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            assert_eq!(
                avb_protocol.write_persistent_value(EXPECTED_NAME, Some(EXPECTED_VALUE)),
                Ok(())
            );
        });
    }

    #[test]
    fn write_persistent_value_delete() {
        const EXPECTED_NAME: &CStr = c"test_key";

        /// C callback implementation for deleting a persistent value.
        ///
        /// # Safety:
        /// * Caller must guarantee that `name` points to a valid null-terminated string.
        unsafe extern "efiapi" fn c_write_persistent_value_delete(
            _: *mut GblEfiAvbProtocol,
            name: *const u8,
            value_size: usize,
            value: *const u8,
        ) -> EfiStatus {
            assert_eq!(
                // SAFETY:
                // * `name` is a valid pointer to null-terminated string.
                unsafe { CStr::from_ptr(name as _) },
                EXPECTED_NAME
            );
            assert!(value.is_null());
            assert_eq!(value_size, 0);

            return EFI_STATUS_SUCCESS;
        }

        let c_interface = GblEfiAvbProtocol {
            write_persistent_value: Some(c_write_persistent_value_delete),
            ..Default::default()
        };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            assert_eq!(avb_protocol.write_persistent_value(EXPECTED_NAME, None), Ok(()));
        });
    }

    #[test]
    fn write_persistent_value_error_handled() {
        const EXPECTED_NAME: &CStr = c"test_key";
        const EXPECTED_VALUE: &[u8] = b"test_value";

        /// C callback implementation that returns an error.
        ///
        /// # Safety:
        /// * Caller must guarantee that `name` points to a valid null-terminated string.
        unsafe extern "efiapi" fn c_write_persistent_value_error(
            _: *mut GblEfiAvbProtocol,
            name: *const u8,
            _: usize,
            _: *const u8,
        ) -> EfiStatus {
            assert_eq!(
                // SAFETY:
                // * `name` is a valid pointer to null-terminated string.
                unsafe { CStr::from_ptr(name as _) },
                EXPECTED_NAME
            );

            return EFI_STATUS_INVALID_PARAMETER;
        }

        let c_interface = GblEfiAvbProtocol {
            write_persistent_value: Some(c_write_persistent_value_error),
            ..Default::default()
        };

        run_test_with_mock_protocol(c_interface, |avb_protocol| {
            assert_eq!(
                avb_protocol.write_persistent_value(EXPECTED_NAME, Some(EXPECTED_VALUE)),
                Err(Error::InvalidInput),
            );
        });
    }
}
