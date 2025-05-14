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

//! Describe common traits and structures for GBL profiling.

use core::{panic::Location, time::Duration};

/// Timer for profiling
pub trait ProfileTimer {
    /// Get the elapsed time since the creation of this timer.
    ///
    /// Note: if there are any errors measuring the elapsed life of the timer,
    ///       the implementation should log the error and return Duration::ZERO.
    fn elapsed(&self) -> Duration;
}

/// Reporter for profiling data.
/// This can be as simple as printing to the console
/// or as sophisticated as adding data to an in-memory structure
/// that will be written to disk or passed to the booting kernel.
pub trait Reporter {
    /// Report profiling information.
    fn report(&self, filename: &'static str, funcname: &'static str, elapsed: Duration);
}

/// Profiler struct that captures the run time of a function invocation.
/// Intended use is to annotate a function with
pub struct Profiler<T: ProfileTimer, R: Reporter> {
    timer: T,
    reporter: R,
    filename: &'static str,
    funcname: &'static str,
}

impl<T: ProfileTimer, R: Reporter> Profiler<T, R> {
    /// Create a new profiler
    #[track_caller]
    pub fn new(timer: T, reporter: R, funcname: &'static str) -> Self {
        let filename = Location::caller().file();
        Self {
            timer,
            filename: filename.strip_prefix("external/gbl/").unwrap_or(filename),
            funcname,
            reporter,
        }
    }
}

impl<T: ProfileTimer, R: Reporter> Drop for Profiler<T, R> {
    fn drop(&mut self) {
        let elapsed = self.timer.elapsed();
        self.reporter.report(self.filename, self.funcname, elapsed);
    }
}

/// Backend for profile timers and reporter handles.
/// The point of this indirection is to hide the types of ProfileTimer and Reporter
/// and avoid leaking dependencies and implementation details.
///
/// Intended use is to annotate target functions with
///
/// #[gbl_profile(backend = <custom backend initialization>)]
/// fn expensive_function(...) { ... }
pub trait ProfileBackend {
    /// Create a new timer.
    fn new_timer(&self) -> impl ProfileTimer;

    /// Get a handle to the reporter implementation.
    fn reporter(&self) -> impl Reporter;
}
