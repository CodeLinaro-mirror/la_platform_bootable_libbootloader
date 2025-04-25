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

//! This file provides implementation of the sysdeps functions for boringssl.
//! Global allocator is required.

#![cfg_attr(not(test), no_std)]

use core::ffi::{c_char, c_int, c_uint, c_void};
use libc::{abort, gbl_free, gbl_malloc, memcmp, memset};

/// int CRYPTO_memcmp(const void *a, const void *b, size_t len);
///
/// Safety
///
/// `src1` and `src2` must each be non‑null and point to memory blocks of at least `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn CRYPTO_memcmp(
    src1: *const c_void,
    src2: *const c_void,
    n: usize,
) -> c_int {
    // SAFETY: GBL libc implementation calls are compatible with libc counterparts
    unsafe { memcmp(src1, src2, n) }
}

/// void *OPENSSL_malloc(size_t size);
///
/// Safety
///
/// The caller must check for a null return to handle allocation failure.
#[no_mangle]
pub unsafe extern "C" fn OPENSSL_malloc(size: usize) -> *mut c_void {
    // SAFETY: GBL libc implementation calls are compatible with libc counterparts
    unsafe { gbl_malloc(size, 8) }
}

/// void OPENSSL_free(void *ptr);
///
/// Safety
///
/// `ptr` must either be null or have been returned by a prior call to `OPENSSL_malloc`
#[no_mangle]
pub unsafe extern "C" fn OPENSSL_free(ptr: *mut c_void) {
    // SAFETY: GBL libc implementation calls are compatible with libc counterparts
    unsafe { gbl_free(ptr, 8) }
}

/// void OPENSSL_cleanse(void *ptr, size_t len);
///
/// BoringSSL provides this in crypto/mem.cc, which is complex to compile for a no_std
/// MSVC clang target. We implement it at the GBL sysdeps level instead.
///
/// Safety
///
/// `dest` must be non-null and point to a writable memory region of at least `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn OPENSSL_cleanse(dest: *mut c_void, n: usize) {
    // SAFETY: GBL libc implementation calls are compatible with libc counterparts
    unsafe { memset(dest, 0, n) };
}

/// int RAND_bytes(uint8_t *buf, size_t len);
///
/// Required by X25519_keypair and ED25519_keypair from crypto/curve25519.cc at linking time
/// but never actually getting used by opendice, so fail if called.
///
/// Safety
///
/// `buf` must be non‑null and point to a writable memory region of at least `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn RAND_bytes(_buf: *mut u8, _len: usize) -> c_int {
    abort()
}

/// void ERR_put_error(int library, int unused, int reason, const char *file, unsigned line)
///
/// Used internally by boringssl to report errors that occur. Since only reporting is used by
/// ed25519/sha implementations, we can keep this as a noop.
///
/// If we later need (i.e to pull ECSDA) related methods like:
/// 1. ERR_peek_error
/// 2. ERR_peek_last_error
/// 3. ERR_clear_error
///    ... and so on,
/// then we’ll have to compile crypto/err/err.cc or provide compatible implementations.
///
/// Safety
///
/// Noop implementation - any usage is safe.
#[no_mangle]
pub unsafe extern "C" fn ERR_put_error(
    _library: c_int,
    _unused: c_int,
    _reason: c_int,
    _file: *const c_char,
    _line: c_uint,
) {
    // no-op
}
