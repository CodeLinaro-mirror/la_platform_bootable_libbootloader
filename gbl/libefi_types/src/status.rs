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

//! EFI status code wrappers.

use crate::defs::*;
use core::num::NonZero;

/// Represents a UEFI error code, i.e. any [EfiStatus] except
/// [EFI_STATUS_SUCCESS].
///
/// The purpose of this is to bridge the C raw enum into a Rust [Result] type.
/// One problem with using [EfiStatus] directly via `Result<T, EfiStatus>` is
/// that it's possible to create `Err(EFI_STATUS_SUCCESS)`, which is
/// non-sensical and likely to cause bugs. With [EfiError], it only holds
/// non-success states, so `Result<T, EfiError>` is always logically consistent.
///
/// Another issue is that the `EFI_BUFFER_TOO_SMALL` error code should always
/// be paired with a size value indicating how large the buffer needs to be.
/// With raw C status codes this either requires using out-params, or pairing
/// a status code with `Option<usize>` that's only valid in the buffer-too-small
/// case, both of which are awkward. Defining an enum here allows more natural
/// pairing of only this particular error code with a size value.
///
/// The naming mirrors the UEFI definitions.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum EfiError {
    CrcError,
    Aborted,
    AccessDenied,
    AlreadyStarted,
    BadBufferSize,
    /// Unlike other errors, this is always accompanied by the size that the
    /// buffer would need in order to succeed.
    BufferTooSmall(usize),
    CompromisedData,
    ConnectionFin,
    ConnectionRefused,
    ConnectionReset,
    DeviceError,
    EndOfFile,
    EndOfMedia,
    HttpError,
    IcmpError,
    IncompatibleVersion,
    InvalidLanguage,
    InvalidParameter,
    IpAddressConflict,
    LoadError,
    MediaChanged,
    NotFound,
    NotReady,
    NotStarted,
    NoMapping,
    NoMedia,
    NoResponse,
    OutOfResources,
    ProtocolError,
    SecurityViolation,
    TftpError,
    Timeout,
    Unsupported,
    VolumeCorrupted,
    VolumeFull,
    WriteProtected,
    /// A custom error or warning; the value is the EFI status code.
    ///
    /// Zero is not permitted since that represents [EFI_STATUS_SUCCESS] which
    /// is not an error.
    ///
    /// An error code that is already one of the [EfiError] enum variants is
    /// allowed, but when passing this code over a UEFI boundary the recipient
    /// will see it as the variant. For example, if you send an `Other` error
    /// with the value `0x8000000000000002` (`INVALID_PARAMETER`), the receiver
    /// will see it as `EfiError::InvalidParameter`.
    Other(NonZero<usize>),
}

impl EfiError {
    /// Returns true if this is a warning code rather than an error code.
    pub fn is_warning(&self) -> bool {
        // All our predefined codes are errors, only `Other` may be a
        // warning, in which case high bit set = error, unset = warning.
        // https://uefi.org/specs/UEFI/2.10/Apx_D_Status_Codes.html.
        matches!(self, Self::Other(code) if code.leading_zeros() > 0)
    }
}

/// Any status other than [EFI_STATUS_SUCCESS] can convert to [EfiError].
///
/// [EFI_STATUS_BUFFER_TOO_SMALL] will default to 0 `usize`.
impl TryFrom<EfiStatus> for EfiError {
    type Error = &'static str;

    fn try_from(status: EfiStatus) -> Result<Self, Self::Error> {
        match status {
            EFI_STATUS_SUCCESS => Err("Cannot convert EFI_STATUS_SUCCESS to an EfiError"),
            EFI_STATUS_CRC_ERROR => Ok(Self::CrcError),
            EFI_STATUS_ABORTED => Ok(Self::Aborted),
            EFI_STATUS_ACCESS_DENIED => Ok(Self::AccessDenied),
            EFI_STATUS_ALREADY_STARTED => Ok(Self::AlreadyStarted),
            EFI_STATUS_BAD_BUFFER_SIZE => Ok(Self::BadBufferSize),
            EFI_STATUS_BUFFER_TOO_SMALL => Ok(Self::BufferTooSmall(0)),
            EFI_STATUS_COMPROMISED_DATA => Ok(Self::CompromisedData),
            EFI_STATUS_CONNECTION_FIN => Ok(Self::ConnectionFin),
            EFI_STATUS_CONNECTION_REFUSED => Ok(Self::ConnectionRefused),
            EFI_STATUS_CONNECTION_RESET => Ok(Self::ConnectionReset),
            EFI_STATUS_DEVICE_ERROR => Ok(Self::DeviceError),
            EFI_STATUS_END_OF_FILE => Ok(Self::EndOfFile),
            EFI_STATUS_END_OF_MEDIA => Ok(Self::EndOfMedia),
            EFI_STATUS_HTTP_ERROR => Ok(Self::HttpError),
            EFI_STATUS_ICMP_ERROR => Ok(Self::IcmpError),
            EFI_STATUS_INCOMPATIBLE_VERSION => Ok(Self::IncompatibleVersion),
            EFI_STATUS_INVALID_LANGUAGE => Ok(Self::InvalidLanguage),
            EFI_STATUS_INVALID_PARAMETER => Ok(Self::InvalidParameter),
            EFI_STATUS_IP_ADDRESS_CONFLICT => Ok(Self::IpAddressConflict),
            EFI_STATUS_LOAD_ERROR => Ok(Self::LoadError),
            EFI_STATUS_MEDIA_CHANGED => Ok(Self::MediaChanged),
            EFI_STATUS_NOT_FOUND => Ok(Self::NotFound),
            EFI_STATUS_NOT_READY => Ok(Self::NotReady),
            EFI_STATUS_NOT_STARTED => Ok(Self::NotStarted),
            EFI_STATUS_NO_MAPPING => Ok(Self::NoMapping),
            EFI_STATUS_NO_MEDIA => Ok(Self::NoMedia),
            EFI_STATUS_NO_RESPONSE => Ok(Self::NoResponse),
            EFI_STATUS_OUT_OF_RESOURCES => Ok(Self::OutOfResources),
            EFI_STATUS_PROTOCOL_ERROR => Ok(Self::ProtocolError),
            EFI_STATUS_SECURITY_VIOLATION => Ok(Self::SecurityViolation),
            EFI_STATUS_TFTP_ERROR => Ok(Self::TftpError),
            EFI_STATUS_TIMEOUT => Ok(Self::Timeout),
            EFI_STATUS_UNSUPPORTED => Ok(Self::Unsupported),
            EFI_STATUS_VOLUME_CORRUPTED => Ok(Self::VolumeCorrupted),
            EFI_STATUS_VOLUME_FULL => Ok(Self::VolumeFull),
            EFI_STATUS_WRITE_PROTECTED => Ok(Self::WriteProtected),
            _ => {
                // unwrap() will always succeed here because 0 would have
                // followed the `EFI_STATUS_SUCCESS` arm above.
                Ok(Self::Other(NonZero::new(status.0).unwrap()))
            }
        }
    }
}

/// Converts an [EfiError] into a raw [EfiStatus].
///
/// If the error is [EfiError::BufferTooSmall], the associated usize
/// will be dropped.
impl From<EfiError> for EfiStatus {
    fn from(error: EfiError) -> Self {
        match error {
            EfiError::CrcError => EFI_STATUS_CRC_ERROR,
            EfiError::Aborted => EFI_STATUS_ABORTED,
            EfiError::AccessDenied => EFI_STATUS_ACCESS_DENIED,
            EfiError::AlreadyStarted => EFI_STATUS_ALREADY_STARTED,
            EfiError::BadBufferSize => EFI_STATUS_BAD_BUFFER_SIZE,
            EfiError::BufferTooSmall(_) => EFI_STATUS_BUFFER_TOO_SMALL,
            EfiError::CompromisedData => EFI_STATUS_COMPROMISED_DATA,
            EfiError::ConnectionFin => EFI_STATUS_CONNECTION_FIN,
            EfiError::ConnectionRefused => EFI_STATUS_CONNECTION_REFUSED,
            EfiError::ConnectionReset => EFI_STATUS_CONNECTION_RESET,
            EfiError::DeviceError => EFI_STATUS_DEVICE_ERROR,
            EfiError::EndOfFile => EFI_STATUS_END_OF_FILE,
            EfiError::EndOfMedia => EFI_STATUS_END_OF_MEDIA,
            EfiError::HttpError => EFI_STATUS_HTTP_ERROR,
            EfiError::IcmpError => EFI_STATUS_ICMP_ERROR,
            EfiError::IncompatibleVersion => EFI_STATUS_INCOMPATIBLE_VERSION,
            EfiError::InvalidLanguage => EFI_STATUS_INVALID_LANGUAGE,
            EfiError::InvalidParameter => EFI_STATUS_INVALID_PARAMETER,
            EfiError::IpAddressConflict => EFI_STATUS_IP_ADDRESS_CONFLICT,
            EfiError::LoadError => EFI_STATUS_LOAD_ERROR,
            EfiError::MediaChanged => EFI_STATUS_MEDIA_CHANGED,
            EfiError::NotFound => EFI_STATUS_NOT_FOUND,
            EfiError::NotReady => EFI_STATUS_NOT_READY,
            EfiError::NotStarted => EFI_STATUS_NOT_STARTED,
            EfiError::NoMapping => EFI_STATUS_NO_MAPPING,
            EfiError::NoMedia => EFI_STATUS_NO_MEDIA,
            EfiError::NoResponse => EFI_STATUS_NO_RESPONSE,
            EfiError::OutOfResources => EFI_STATUS_OUT_OF_RESOURCES,
            EfiError::ProtocolError => EFI_STATUS_PROTOCOL_ERROR,
            EfiError::SecurityViolation => EFI_STATUS_SECURITY_VIOLATION,
            EfiError::TftpError => EFI_STATUS_TFTP_ERROR,
            EfiError::Timeout => EFI_STATUS_TIMEOUT,
            EfiError::Unsupported => EFI_STATUS_UNSUPPORTED,
            EfiError::VolumeCorrupted => EFI_STATUS_VOLUME_CORRUPTED,
            EfiError::VolumeFull => EFI_STATUS_VOLUME_FULL,
            EfiError::WriteProtected => EFI_STATUS_WRITE_PROTECTED,
            EfiError::Other(code) => EfiStatus(code.get()),
        }
    }
}

/// Common result alias to reduce boilerplate.
pub type EfiResult<T> = Result<T, EfiError>;

/// Converts a raw [EfiStatus] into an [EfiResult].
///
/// [EFI_STATUS_BUFFER_TOO_SMALL] will default to 0 `usize`.
impl From<EfiStatus> for EfiResult<()> {
    fn from(status: EfiStatus) -> Self {
        match status {
            EFI_STATUS_SUCCESS => Ok(()),
            // unwrap() will always succeed here because we handled the only
            // possible failure case above.
            _ => Err(status.try_into().unwrap()),
        }
    }
}

/// Converts an [EfiResult] into a raw [EfiStatus].
///
/// If the result is [EfiError::BufferTooSmall], the associated usize
/// will be dropped.
impl<T> From<EfiResult<T>> for EfiStatus {
    fn from(result: EfiResult<T>) -> Self {
        match result {
            Ok(_) => EFI_STATUS_SUCCESS,
            Err(e) => e.into(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn efi_error_try_from_status_success() {
        assert_eq!(EfiError::try_from(EFI_STATUS_ACCESS_DENIED).unwrap(), EfiError::AccessDenied);
    }

    #[test]
    fn efi_error_try_from_status_other_success() {
        assert_eq!(
            // 500 is not a defined EFI error code so should go to `Other`.
            EfiError::try_from(EfiStatus(500)).unwrap(),
            EfiError::Other(NonZero::new(500).unwrap())
        );
    }

    #[test]
    fn efi_error_try_from_status_failure() {
        // Can't turn SUCCESS into an error.
        assert!(EfiError::try_from(EFI_STATUS_SUCCESS).is_err());
    }

    #[test]
    fn efi_error_other_converts_to_actual_variant() {
        // Create an `Other` error but with an actual known error code.
        let error = EfiError::Other(NonZero::new(EFI_STATUS_INVALID_PARAMETER.0).unwrap());
        // Convert to raw, as it would when transferring over the UEFI C API.
        let raw_error = EfiStatus::from(error);
        // Back to `EfiResult` as the UEFI recipient would.
        let result = EfiResult::from(raw_error);

        // The final conversion has snapped back to the known variant, is no longer `Other`.
        assert_eq!(result, Err(EfiError::InvalidParameter));
    }

    #[test]
    fn efi_error_is_warning() {
        // First bit unset should be recognized as a warning code.
        let error = EfiError::Other(NonZero::new(5).unwrap());
        assert!(error.is_warning());
    }

    #[test]
    fn efi_error_is_not_warning() {
        assert!(!EfiError::NoMedia.is_warning());
    }

    #[test]
    fn efi_result_from_success() {
        assert_eq!(EfiResult::from(EFI_STATUS_SUCCESS), Ok(()));
    }

    #[test]
    fn efi_result_from_error() {
        assert_eq!(EfiResult::from(EFI_STATUS_INVALID_PARAMETER), Err(EfiError::InvalidParameter));
    }

    #[test]
    fn success_from_efi_result() {
        assert_eq!(EfiStatus::from(Ok(())), EFI_STATUS_SUCCESS);
    }

    #[test]
    fn error_from_efi_result() {
        assert_eq!(EfiStatus::from(Err::<(), _>(EfiError::NotFound)), EFI_STATUS_NOT_FOUND);
    }
}
