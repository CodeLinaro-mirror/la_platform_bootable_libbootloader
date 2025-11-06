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

//! Helpers for obtaining random data via [GblOps].

use crate::{ops::RngAlgorithm, GblOps};
use liberror::{Error, Result};

/// Helper to detect recoverable RNG errors, where retrying with another
/// algorithm may succeed.
pub fn should_fallback_to_default_rng(e: Error) -> bool {
    matches!(e, Error::Unsupported | Error::NotReady)
}

/// Helper function to obtain a random seed, preferring raw hardware entropy, with a
/// fallback to the default RNG algorithm if needed. Raw seed is more suitable for
/// initializing other random data.
pub fn get_random_seed<'a, 'b>(ops: &mut impl GblOps<'a, 'b>, buffer: &mut [u8]) -> Result<()> {
    // Try to obtain raw hardware entropy.
    match ops.get_random_bytes(RngAlgorithm::Raw, buffer) {
        Ok(_) => Ok(()),
        // Fallback to generated random data which is acceptable.
        Err(e) if should_fallback_to_default_rng(e) => {
            ops.get_random_bytes(RngAlgorithm::Default, buffer)
        }
        Err(e) => Err(e),
    }
}
