/*
 * Copyright (C) 2025 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 */

#ifndef __EFI_SHA_C_H__
#define __EFI_SHA_C_H__

#include "libavb/avb_sha.h"

#ifdef __cplusplus
extern "C" {
#endif

/// Allocates and initializes a Hasher<Sha256> and returns its pointer.
/// This is an opaque pointer for the caller.
/// Ownership of the Hasher<Sha256> is passed to the caller.
void* efi_sha256_init();

/// Adds data to the Hasher<Sha256> context.
///
/// Safety:
/// * `hasher` points to a valid Hasher<Sha256> that has been initialized.
/// * `data` is valid to read for `len` bytes.
void efi_sha256_update(void* hasher, uint8_t const* data, size_t len);

/// Returns the digest from a Hasher<Sha256>.
///
/// Safety:
/// * `hasher_ptr` points to a pointer to a valid Hasher<Sha256> that has been
/// initialized.
/// * `out` is valid to write for 32 bytes.
void efi_sha256_final(void** hasher, uint8_t out[AVB_SHA256_DIGEST_SIZE]);

/// Allocates and initializes a Hasher<Sha512> and returns its pointer.
/// This is an opaque pointer for the caller.
/// Ownership of the Hasher<Sha512> is passed to the caller.
void* efi_sha512_init();

/// Adds data to the Hasher<Sha512> context.
///
/// Safety:
/// * `hasher` points to a valid Hasher<Sha512> that has been initialized.
/// * `data` is valid to read for `len` bytes.
void efi_sha512_update(void* hasher, uint8_t const* data, size_t len);

/// Returns the digest from a Hasher<Sha512>.
///
/// Safety:
/// * `hasher_ptr` points to a pointer to a valid Hasher<Sha512> that has been
/// initialized.
/// * `out` is valid to write for 32 bytes.
void efi_sha512_final(void** hasher, uint8_t out[AVB_SHA512_DIGEST_SIZE]);

#ifdef __cplusplus
}
#endif

#endif  // __EFI_SHA_C_H__
