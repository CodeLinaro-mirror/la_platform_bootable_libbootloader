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

load("@rules_license//rules:license.bzl", "license")
load("@rules_license//rules:license_kind.bzl", "license_kind")
load(":readme.bzl", "readme_test")

package(
    default_visibility = ["//visibility:public"],
)

# GBL code uses Apache 2.0.
license(
    name = "gbl_license",
    license_kind = "@rules_license//licenses/spdx:Apache-2.0",
    license_text = "LICENSE",
    visibility = ["//visibility:public"],
)

# Android has a MODULE_LICENSE_BSD_LIKE marker which doesn't map directly to any
# particular license but can be treated as a general notice-type license.
#
# For GBL we only allow notice-type licenses so we don't need any explicit
# metadata indicating this (the upstream rules_license licenses do not provide
# this information either so this is consistent).
license_kind(
    name = "BSD-like",
)

readme_test(
    name = "readme_test",
    readme = "docs/efi_protocols.md",
    # Note: can't read files in subpackages, not sure how to add a dependency on all,
    # globbed, subpackages and all the rust_lib targets they contain.
    deps = [
        "@gbl//efi:app",
        "@gbl//efi:libgbl_efi",
        "@gbl//libasync",
        "@gbl//libavb:sysdeps",
        "@gbl//libboot",
        "@gbl//libbootimg",
        "@gbl//libbootparams",
        "@gbl//libc",
        "@gbl//libefi",
        "@gbl//libefi_types",
        "@gbl//liberror",
        "@gbl//libfastboot",
        "@gbl//libfdt",
        "@gbl//libgbl",
        "@gbl//libmisc",
        "@gbl//libsafemath",
        "@gbl//libstorage",
        "@gbl//libutils",
    ],
)
