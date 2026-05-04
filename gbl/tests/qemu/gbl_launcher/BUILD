# Copyright (C) 2026 The Android Open Source Project
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

load("@gbl//toolchain:gbl_toolchain.bzl", "build_with_platform")
load("@rules_rust//rust:defs.bzl", "rust_binary")

package(
    default_applicable_licenses = ["@gbl//licensing:gbl_license"],
    default_visibility = ["//visibility:public"],
)

rust_binary(
    name = "gbl_launcher_bin",
    srcs = ["src/main.rs"],
    edition = "2024",
    rustc_flags = [
        "-C",
        "panic=abort",
    ],
    deps = [
        "@avb//:libavb_c_sha_staticlib",
        "@boringssl//:libcrypto_c_baremetal_static",
        "@gbl//libavb:sysdeps",
        "@gbl//libboringssl:sysdeps",
        "@gbl//libefi",
        "@gbl//libefi_types",
        "@gbl//libsemihosting",
        "@gbl//libstack",
    ],
)

build_with_platform(
    name = "gbl_launcher_aarch64",
    platform = "@gbl//toolchain:gbl_uefi_aarch64_dev",
    deps = [":gbl_launcher_bin"],
)
