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

//! Exports the BUILD_NUMBER environment variable.

#![cfg_attr(not(test), no_std)]

/// The string describing the build ID or build number.
pub const BUILD_NUMBER: &str = env!("BUILD_NUMBER", "BUILD_NUMBER must be defined");

/// The string describing the GBL version release.
pub const VERSION: &str = "mainline-18";

/// The string describing the branch this is built on.
pub const BUILD_BRANCH: &str = match option_env!("BUILD_BRANCH") {
    Some(branch) if !branch.is_empty() => branch,
    _ => "<unknown_branch>",
};

/// The git revision of the gbl repo.
pub const BUILD_REVISION: &str = match option_env!("BUILD_REVISION") {
    Some(revision) if !revision.is_empty() => revision,
    _ => "<unknown_revision>",
};
