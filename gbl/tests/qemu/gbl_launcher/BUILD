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

# TODO(b/509953349): Remove this once kernel is assembled into boot image.
# Copy test kernel to the launcher directory, so that include_bytes! can find it. Otherwise the
# compiled data could be in an arbitrary sandbox location.
genrule(
    name = "copy_test_kernel",
    srcs = ["@gbl//tests/qemu/test_kernel:test_kernel_aarch64"],
    outs = ["test_kernel.bin"],
    cmd = "cp $< $@",
)

rust_binary(
    name = "gbl_launcher_bin",
    srcs = ["src/main.rs"],
    compile_data = [
        # TODO(b/509953349): Remove this once kernel is assembled into boot image.
        ":copy_test_kernel",
    ],
    edition = "2024",
    rustc_flags = [
        "-C",
        "panic=abort",
    ],
    deps = [
        "@avb//:libavb_c_sha_staticlib",
        "@gbl//libavb:sysdeps",
        "@gbl//libboot",
        "@gbl//libboringssl:libcrypto_c_baremetal_static",
        "@gbl//libboringssl:sysdeps",
        "@gbl//libefi",
        "@gbl//libefi:libefi_rng",
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
