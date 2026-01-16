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

load("@rules_license//rules:license.bzl", "license")
load("@rules_license//rules:license_kind.bzl", "license_kind")

package(
    default_visibility = ["//visibility:public"],
)

# GBL code uses Apache 2.0.
license(
    name = "gbl_license",
    license_kind = "@rules_license//licenses/spdx:Apache-2.0",
    license_text = "LICENSE",
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
