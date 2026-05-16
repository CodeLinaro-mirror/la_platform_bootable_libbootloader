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

load("@gbl//toolchain:gbl_toolchain.bzl", "link_static_cc_library")
load("@gbl//toolchain:gbl_workspace_util.bzl", "ANDROID_RUST_LINTS")
load("@rules_cc//cc:defs.bzl", "cc_library")
load("@rules_rust//rust:defs.bzl", "rust_library")
load("@rules_rust_bindgen//:defs.bzl", "rust_bindgen")

package(
    default_applicable_licenses = ["@gbl//licensing:gbl_license"],
    default_visibility = ["//visibility:public"],
)

exports_files(glob(["**/*"]))

rust_library(
    name = "sysdeps",
    srcs = glob(["**/*.rs"]),
    crate_name = "boringssl_sysdeps",
    edition = "2021",
    rustc_flags = ANDROID_RUST_LINTS,
    deps = [
        "@gbl//libc",
    ],
)

link_static_cc_library(
    name = "libcrypto_c_baremetal_static",
    cc_library = "@boringssl//:crypto_baremetal",
)

# Additional stub headers to make bindgen work with wrapper.h
cc_library(
    name = "crypto_with_stubs",
    hdrs = glob(["include/**/*.h"]),
    includes = ["include"],
    deps = [
        "@boringssl//:crypto_baremetal",
    ],
)

rust_bindgen(
    name = "bssl_sys_bindgen",
    bindgen_flags = [
        "--use-core",
        "--no-layout-tests",
        "--raw-line=#![no_std]",
        "--raw-line=#![allow(missing_docs)]",
        "--raw-line=#![allow(non_camel_case_types)]",
        "--raw-line=#![allow(non_snake_case)]",
        "--raw-line=#![allow(non_upper_case_globals)]",
        "--ctypes-prefix=core::ffi",
    ],
    cc_lib = ":crypto_with_stubs",
    header = "@boringssl//:src/rust/bssl-sys/wrapper.h",
)

rust_library(
    name = "bssl-sys-raw",
    srcs = [":bssl_sys_bindgen"],
    crate_name = "bssl_sys_raw",
    edition = "2021",
    rustc_flags = ANDROID_RUST_LINTS,
)

rust_library(
    name = "bssl-sys",
    srcs = ["src/bssl_sys.rs"],
    crate_name = "bssl_sys",
    edition = "2021",
    rustc_flags = ANDROID_RUST_LINTS,
    deps = [
        ":bssl-sys-raw",
    ],
)

rust_library(
    name = "bssl-crypto",
    srcs = ["@boringssl//:rust_bssl_crypto_sources"],
    crate_name = "bssl_crypto",
    edition = "2021",
    rustc_flags = ANDROID_RUST_LINTS,
    deps = [
        ":bssl-sys",
    ],
)
