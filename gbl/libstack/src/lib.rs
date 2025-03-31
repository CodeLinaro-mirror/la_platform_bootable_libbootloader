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

//! Set up global values for stack protector machinery.

#![no_std]

use efi_types::EfiSystemTable;

unsafe extern "C" {
    /// Set the global canary value and pass a pointer to the system table
    /// to stack check functions.
    ///
    /// The stack protector functions make direct EFI calls to
    /// Simple Text Output Protocol and Runtime Services to avoid
    /// infinite recursion.
    ///
    /// Note: Different platforms have slightly different (badly documented)
    /// canary semantics, but the following guidelines may be helpful:
    /// * The leftmost byte (and probably the second leftmost byte too) should be 00.
    ///   This helps protect against format string attacks and printing
    ///   caller stack frames by providing a last-ditch string null terminator.
    /// * The other bytes should be set to a random value at the very beginning of
    ///   program execution.
    ///
    /// Warning: A compile-time static canary is straightforward to bypass.
    ///
    /// # Safety
    ///
    /// * `systab` must be a valid non-NULL pointer.
    /// * The caller MUST NOT RETURN.
    pub unsafe fn initialize_canary(systab: *mut EfiSystemTable, canary: usize);
}
