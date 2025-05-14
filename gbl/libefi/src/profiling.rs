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

//! EFI backed implementation of GBL profiling framework.
//!
//! Note: profiling support requires the Efi Timestamp Protocol.
//!       If this protocol is not available the profilier will not
//!       crash the bootloader, but all output from the profiling machinery
//!       will only indicate errors.

use crate::protocol::timestamp::TimestampProtocol;
use crate::{efi_println, EfiEntry};
use core::time::Duration;
use efi_types::EfiTimestampProperties;
use liberror::Result;
use libgbl::profiling::{ProfileBackend, ProfileTimer, Reporter};

/// Represents a timestamp obtained using EFI_TIMESTAMP_PROTOCOL.
pub struct Timestamp(Result<u64>);

impl Timestamp {
    /// Creates a new instance from the current timestamp.
    pub fn now(entry: &EfiEntry) -> Self {
        entry
            .system_table()
            .boot_services()
            .find_first_and_open::<TimestampProtocol>()
            .inspect_err(|_| efi_println!(entry, "EFI_TIMESTAMP_PROTOCOL not supported"))
            .and_then(|v| v.get_timestamp())
            .inspect_err(|e| efi_println!(entry, "Error getting timestamp: {}", e))
            .into()
    }
}

impl From<Result<u64>> for Timestamp {
    fn from(r: Result<u64>) -> Self {
        Self(r)
    }
}

/// EFI backed profiling timer
pub struct EfiProfileTimer<'a> {
    start: Timestamp,
    efi_entry: &'a EfiEntry,
}

impl<'a> EfiProfileTimer<'a> {
    /// Construct a new profile timer that starts at the current timestamp.
    pub fn new(efi_entry: &'a EfiEntry) -> Self {
        Self { start: Timestamp::now(efi_entry), efi_entry }
    }

    fn elapsed_helper(&self) -> Result<Duration> {
        let start = self.start.0?;
        let entry = self.efi_entry;
        let protocol = entry
            .system_table()
            .boot_services()
            .find_first_and_open::<TimestampProtocol>()
            .inspect_err(|_| efi_println!(entry, "EFI_TIMESTAMP_PROTOCOL not supported"))?;
        let EfiTimestampProperties { frequency, end_value } = protocol
            .get_properties()
            .inspect_err(|e| efi_println!(entry, "Error getting timestamp properties: {}", e))?;
        let now = protocol
            .get_timestamp()
            .inspect_err(|e| efi_println!(entry, "Error getting timestamp: {}", e))?;
        let delta = now.overflowing_sub(start).0 as u128 % (end_value as u128 + 1);
        Ok(Duration::from_millis((delta as u64) * 1000 / frequency))
    }
}

impl<'a> ProfileTimer for EfiProfileTimer<'a> {
    fn elapsed(&self) -> Duration {
        self.elapsed_helper().unwrap_or(Duration::ZERO)
    }
}

/// Profiling reporter that prints to EFI console.
pub struct ConsoleProfileReporter<'a> {
    efi_entry: &'a EfiEntry,
}

impl<'a> ConsoleProfileReporter<'a> {
    /// Create a new console reporter using the EfiEntry.
    pub fn new(efi_entry: &'a EfiEntry) -> Self {
        Self { efi_entry }
    }
}

impl<'a> Reporter for ConsoleProfileReporter<'a> {
    fn report(&self, filename: &'static str, funcname: &'static str, elapsed: Duration) {
        efi_println!(self.efi_entry, "{}:{}: {}ms", filename, funcname, elapsed.as_millis())
    }
}

/// Backend for EFI backed profiling structures.
pub struct EfiProfileBackend<'a> {
    efi_entry: &'a EfiEntry,
}

impl<'a> EfiProfileBackend<'a> {
    /// Make a new profiler backend.
    pub fn new(efi_entry: &'a EfiEntry) -> Self {
        Self { efi_entry }
    }
}

impl ProfileBackend for EfiProfileBackend<'_> {
    fn new_timer(&self) -> impl ProfileTimer {
        EfiProfileTimer::new(self.efi_entry)
    }

    fn reporter(&self) -> impl Reporter {
        ConsoleProfileReporter::new(self.efi_entry)
    }
}
