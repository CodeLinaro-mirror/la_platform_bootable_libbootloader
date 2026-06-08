# Copyright (C) 2025 The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

load("@gbl//toolchain:gbl_workspace_util.bzl", "ANDROID_RUST_LINTS")
load("@gbl_config//:variables.bzl", "BUILD_NUMBER")
load("@rules_rust//rust:defs.bzl", "rust_library")

package(
    default_applicable_licenses = ["@gbl//licensing:gbl_license"],
)

rust_library(
    name = "libbuild_number",
    srcs = glob(["src/**/*.rs"]),
    edition = "2021",
    rustc_env = {
        "BUILD_NUMBER": BUILD_NUMBER,
    },
    rustc_flags = ANDROID_RUST_LINTS,
    visibility = ["//visibility:public"],
)
