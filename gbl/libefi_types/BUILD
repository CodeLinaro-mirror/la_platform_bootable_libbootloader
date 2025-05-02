# Copyright (C) 2024 The Android Open Source Project
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
load("@gbl_llvm_prebuilts//:info.bzl", "LLVM_PREBUILTS_C_INCLUDE")
load("@rules_cc//cc:cc_library.bzl", "cc_library")
load("@rules_rust//rust:defs.bzl", "rust_library", "rust_test")
load("@rules_rust_bindgen//:defs.bzl", "rust_bindgen")

package(
    default_visibility = ["//visibility:public"],
)

# Newer version of `rust_bindgen` requires a `cc_library` target that actually produces a static
# library and instead of only headers. Thus we generate a placeholder source file to meet the
# requirement.
genrule(
    name = "bindgen_noop_cc",
    outs = ["bindgen_noop_cc.cc"],
    cmd = "touch $(OUTS)",
)

cc_library(
    name = "efi_c_headers",
    srcs = [":bindgen_noop_cc"],
    hdrs = glob(["defs/**/*.h"]),
    includes = ["defs"],
)

rust_bindgen(
    name = "efi_defs_bindgen",
    bindgen_flags = [
        "--ctypes-prefix",
        "core::ffi",
        "--use-core",
        "--with-derive-partialeq",
        "--with-derive-default",
        "--with-derive-custom-struct=EfiMemoryDescriptor=Immutable,IntoBytes,FromBytes,KnownLayout",
        "--with-derive-custom-struct=EfiMemoryAttributesTableHeader=Immutable,IntoBytes,FromBytes,KnownLayout",
        "--override-abi",
        ".*=efiapi",
        "--allowlist-type",
        "(Efi.*)|(GblEfi.*)|(GBL_EFI_.*)",
        "--allowlist-var",
        "PARTITION_NAME_LEN_U16|EFI_.*|GBL_.*",
        "--raw-line",
        """
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(missing_docs)]

use zerocopy::{Immutable, IntoBytes, FromBytes, KnownLayout};""",
    ],
    cc_lib = ":efi_c_headers",
    clang_flags = [
        "-I{}".format(LLVM_PREBUILTS_C_INCLUDE),
        "-nostdinc",
    ],
    header = "defs/efi.h",
)

# Copy the generated source to "src/defs.rs" in the output assembly, so that libefi can include it
# as a source.
genrule(
    name = "efi_defs_genrule",
    srcs = [":efi_defs_bindgen"],
    outs = ["src/defs.rs"],
    cmd = "cat $(location :efi_defs_bindgen) > $@",
)

rust_library(
    name = "libefi_types",
    srcs = glob(["**/*.rs"]) + ["src/defs.rs"],
    crate_name = "efi_types",
    data = [":efi_defs_genrule"],
    rustc_flags = ANDROID_RUST_LINTS,
    deps = [
        "@zerocopy",
    ],
)

rust_test(
    name = "libefi_types_test",
    crate = ":libefi_types",
    rustc_flags = ANDROID_RUST_LINTS,
)
