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

//! Mock profiling.

use crate::MockEfiEntry;
use core::time::Duration;
use libprofile::{ProfileBackend, ProfileTimer, Reporter};
use mockall::mock;

mock! {
    /// Mock [efi::profiling::EfiProfileTimer].
    pub EfiProfileTimer {
        /// Creates a new [MockEfiProfileTimer]
        pub fn new(efi_entry: &MockEfiEntry) -> Self;
    }

    impl ProfileTimer for EfiProfileTimer {
        fn elapsed(&self) -> Duration;
    }
}

/// Mock `EfiProfileTimer` type.
pub type EfiProfileTimer = MockEfiProfileTimer;

mock! {
    /// Mock [efi::profiling::ConsoleProfileReporter]
    pub ConsoleProfileReporter {
        /// Creates a new [efi::profiling::ConsoleProfileReporter]
        pub fn new(efi_entry: &MockEfiEntry) -> Self;
    }

    impl Reporter for ConsoleProfileReporter {
        fn report(&self,
                  filename: &'static str,
                  funcname: &'static str,
                  elapsed: Duration);
    }
}

/// Mock `ConsoleProfileReporter` type.
pub type ConsoleProfileReporter = MockConsoleProfileReporter;

/// Mock `EfiProfileBackend` type
pub struct EfiProfileBackend<'a> {
    efi_entry: &'a MockEfiEntry,
}

impl<'a> EfiProfileBackend<'a> {
    /// Creates a new [efi::profiling::EfiProfileBackend]
    pub fn new(efi_entry: &'a MockEfiEntry) -> Self {
        Self { efi_entry }
    }
}

impl<'a> ProfileBackend for EfiProfileBackend<'a> {
    fn new_timer(&self) -> impl ProfileTimer {
        EfiProfileTimer::new(self.efi_entry)
    }

    fn reporter(&self) -> impl Reporter {
        ConsoleProfileReporter::new(self.efi_entry)
    }
}
