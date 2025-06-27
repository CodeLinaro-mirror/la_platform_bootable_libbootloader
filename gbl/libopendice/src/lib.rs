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

//! This is a wrapper for libopendice.

// TODO: As this was pulled in from Virtualization (libs/dice/open_dice/src/bcc.rs), move common
// Rust wrapper code to external open dice repo.

#![cfg_attr(not(test), no_std)]

use core::ffi::CStr;
use core::ptr;
use opendice_android_bindgen::{
    DiceAndroidConfigValues, DiceAndroidFormatConfigDescriptor, DiceAndroidHandoverMainFlow,
    DICE_ANDROID_CONFIG_COMPONENT_NAME, DICE_ANDROID_CONFIG_COMPONENT_VERSION,
    DICE_ANDROID_CONFIG_RESETTABLE, DICE_ANDROID_CONFIG_RKP_VM_MARKER,
    DICE_ANDROID_CONFIG_SECURITY_VERSION,
};

mod clear_memory;

pub mod dice;
use dice::{context, InputValues};

pub use dice::dice_android_main_flow;

pub mod error;
use error::{check_result, Result};

/// Executes the main Android DICE handover flow.
///
/// A handover combines the DICE chain and CDIs in a single CBOR object.
/// This function takes the current boot stage's handover bundle and produces a
/// bundle for the next stage.
pub fn dice_android_handover_main_flow(
    current_handover: &[u8],
    input_values: &InputValues,
    next_handover: &mut [u8],
) -> Result<usize> {
    let mut next_handover_size = 0;
    check_result(
        // SAFETY: The function only reads `current_handover` and writes to `next_handover`
        // within its bounds,
        // It also reads `input_values` as a constant input and doesn't store any pointer.
        unsafe {
            DiceAndroidHandoverMainFlow(
                context(),
                current_handover.as_ptr(),
                current_handover.len(),
                input_values.as_ptr(),
                next_handover.len(),
                next_handover.as_mut_ptr(),
                &mut next_handover_size,
            )
        },
        next_handover_size,
    )?;

    Ok(next_handover_size)
}

/// Contains the input values used to construct the Android Profile for DICE
/// configuration descriptor.
#[derive(Debug, Default)]
pub struct DiceConfigValues<'a> {
    /// Name of the component.
    pub component_name: Option<&'a CStr>,
    /// Version of the component.
    pub component_version: Option<u64>,
    /// Whether the key changes on factory reset.
    pub resettable: bool,
    /// Monotonically increasing version of the component.
    pub security_version: Option<u64>,
    /// Whether the component can take part in running the RKP VM.
    pub rkp_vm_marker: bool,
}

/// Formats a configuration descriptor following the Android Profile for DICE specification.
/// See <https://pigweed.googlesource.com/open-dice/+/refs/heads/main/docs/android.md>.
pub fn bcc_format_config_descriptor(values: &DiceConfigValues, buffer: &mut [u8]) -> Result<usize> {
    let mut configs = 0;

    let component_name = values.component_name.map_or(ptr::null(), |name| {
        configs |= DICE_ANDROID_CONFIG_COMPONENT_NAME;
        name.as_ptr()
    });
    let component_version = values.component_version.map_or(0, |version| {
        configs |= DICE_ANDROID_CONFIG_COMPONENT_VERSION;
        version
    });
    if values.resettable {
        configs |= DICE_ANDROID_CONFIG_RESETTABLE;
    }
    let security_version = values.security_version.map_or(0, |version| {
        configs |= DICE_ANDROID_CONFIG_SECURITY_VERSION;
        version
    });
    if values.rkp_vm_marker {
        configs |= DICE_ANDROID_CONFIG_RKP_VM_MARKER;
    }

    let values =
        DiceAndroidConfigValues { configs, component_name, component_version, security_version };

    let mut buffer_size = 0;
    check_result(
        // SAFETY: The function writes to the buffer, within the given bounds, and only reads the
        // input values. It writes its result to buffer_size.
        unsafe {
            DiceAndroidFormatConfigDescriptor(
                &values,
                buffer.len(),
                buffer.as_mut_ptr(),
                &mut buffer_size,
            )
        },
        buffer_size,
    )?;
    Ok(buffer_size)
}
