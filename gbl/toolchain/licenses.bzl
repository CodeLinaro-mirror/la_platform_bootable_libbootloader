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

"""License utilities for GBL.

Android packages typically provide licensing information to the build system
via an Android.bp build file, but since we use Bazel instead we need a different
way to specify license information.

Importantly, we should avoid hardcoding per-package license information here,
as hardcoded licenses could get out-of-sync if a package uprev changes the
license information. These utilities instead examine the package files to
determine the correct licensing at build time.
"""

load("@rules_license//rules:license.bzl", "license")

# Android 3P packages indicate licensing via empty marker files whose names
# reflect the license used. This map converts from the Android marker file to
# the corresponding Bazel license rule.
LICENSE_MAP = {
    "MODULE_LICENSE_APACHE2": "@rules_license//licenses/spdx:Apache-2.0",
    "MODULE_LICENSE_MIT": "@rules_license//licenses/spdx:MIT",
    "MODULE_LICENSE_PERMISSIVE": "@rules_license//licenses/generic:permissive",
    "MODULE_LICENSE_ZERO_BSD": "@rules_license//licenses/spdx:0BSD",
}

def generate_license(name = "license", license_text = "LICENSE"):
    """Generates a license() rule by detecting MODULE_LICENSE_* files.

    Args:
        name (String): name of the license rule to generate.
        license_text (String): name of the file containing the license text.
    """

    # Locate the license marker files and convert them to Bazel licenses.
    license_kinds = []
    marker_files = native.glob(["MODULE_LICENSE_*"])
    for marker in marker_files:
        # Ignore any unmapped license type - some packages may be
        # dual-licensed, in which case we elect to use the supported license.
        if marker in LICENSE_MAP:
            license_kinds.append(LICENSE_MAP[marker])

    if not license_kinds:
        fail("{}: No known license kind found in markers: {}".format(native.repo_name(), marker_files))

    if not native.glob([license_text]):
        fail("{}: License file '{}' not found".format(native.repo_name(), license_text))

    license(
        name = name,
        license_kinds = license_kinds,
        license_text = license_text,
        visibility = ["//visibility:public"],
    )
