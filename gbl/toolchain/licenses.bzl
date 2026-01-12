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
load("@rules_license//rules:providers.bzl", "LicenseInfo")

# Android 3P packages indicate licensing via empty marker files whose names
# reflect the license used. This map converts from the Android marker file to
# the corresponding Bazel license rule.
LICENSE_MAP = {
    "MODULE_LICENSE_APACHE2": "@rules_license//licenses/spdx:Apache-2.0",
    "MODULE_LICENSE_MIT": "@rules_license//licenses/spdx:MIT",
    "MODULE_LICENSE_PERMISSIVE": "@rules_license//licenses/generic:permissive",
    "MODULE_LICENSE_ZERO_BSD": "@rules_license//licenses/spdx:0BSD",
}

def generate_license(name = "license", license_text = "LICENSE", bsd_type = None):
    """Generates a license() rule by detecting MODULE_LICENSE_* files.

    Args:
        name (String): name of the license rule to generate.
        license_text (String): name of the file containing the license text.
        bsd_type (optional String): if the repo uses the generic
            MODULE_LICENSE_BSD marker, specify the exact Bazel license kind.
    """

    # Locate the license marker files and convert them to Bazel licenses.
    license_kinds = []
    marker_files = native.glob(["MODULE_LICENSE_*"])
    for marker in marker_files:
        # Ignore any unmapped license type - some packages may be
        # dual-licensed, in which case we elect to use the supported license.
        if marker in LICENSE_MAP:
            license_kinds.append(LICENSE_MAP[marker])
        elif marker == "MODULE_LICENSE_BSD":
            if bsd_type:
                license_kinds.append(bsd_type)

                # Mark the bsd_type as None to indicate it's been used.
                bsd_type = None
            else:
                fail("{}: Must specify `bsd_type` for MODULE_LICENSE_BSD".format(native.repo_name()))

    if not license_kinds:
        fail("{}: No known license kind found in markers: {}".format(native.repo_name(), marker_files))

    # If bsd_type is still valid here, it means the caller passed it but the
    # MODULE_LICENSE_BSD marker file doesn't exist - something may have changed
    # in the licensing, error out here to alert us to re-examine the call site.
    if bsd_type:
        fail("{}: `bsd_type` was provided but no MODULE_LICENSE_BSD file exists: {}".format(native.repo_name()))

    if not native.glob([license_text]):
        fail("{}: License file '{}' not found".format(native.repo_name(), license_text))

    license(
        name = name,
        license_kinds = license_kinds,
        license_text = license_text,
        visibility = ["//visibility:public"],
    )

LicenseCheckInfo = provider(
    "Accumulates the names of all dependencies with missing license information",
    fields = {
        "missing_licenses": "Depset of strings representing targets missing licenses",
    },
)

def _check_licenses_aspect_impl(target, ctx):
    missing = []
    transitive = []

    # If it's not a rule (e.g. source file), we skip checking it directly
    if hasattr(ctx, "rule"):
        # Skip filegroups as they are just groupings of files and don't need
        # licenses themselves.
        if ctx.rule.kind != "filegroup":
            # Accept either a target license or a package default license.
            applicable_licenses = getattr(ctx.rule.attr, "applicable_licenses", [])
            package_metadata = getattr(ctx.rule.attr, "package_metadata", [])

            has_license = False
            for dep in applicable_licenses + package_metadata:
                if LicenseInfo in dep:
                    has_license = True
                    break

            if not has_license:
                missing.append(str(target.label))

        # Collect transitive info from all dependencies.
        for attr_name in dir(ctx.rule.attr):
            # Skip private attributes.
            if attr_name.startswith("_"):
                continue

            attr = getattr(ctx.rule.attr, attr_name)

            # Attr can be a list (e.g. `deps`) or a single label (`target`).
            if type(attr) == "list":
                for item in attr:
                    if type(item) == "Target" and LicenseCheckInfo in item:
                        transitive.append(item[LicenseCheckInfo].missing_licenses)
            elif type(attr) == "Target" and LicenseCheckInfo in attr:
                transitive.append(attr[LicenseCheckInfo].missing_licenses)

    return [LicenseCheckInfo(
        missing_licenses = depset(missing, transitive = transitive),
    )]

check_licenses_aspect = aspect(
    implementation = _check_licenses_aspect_impl,
    attr_aspects = ["*"],
    doc = "Aspect that collects targets with missing licenses.",
)

def _check_licenses_rule_impl(ctx):
    all_missing_depsets = []
    for dep in ctx.attr.deps:
        if LicenseCheckInfo in dep:
            all_missing_depsets.append(dep[LicenseCheckInfo].missing_licenses)

    all_missing = depset(transitive = all_missing_depsets).to_list()

    if all_missing:
        # Sort for deterministic output
        sorted_missing = sorted(all_missing)
        fail("The following targets are missing applicable_licenses:\n" + "\n".join(["  " + str(m) for m in sorted_missing]))

    return [DefaultInfo()]

check_licenses = rule(
    implementation = _check_licenses_rule_impl,
    attrs = {
        "deps": attr.label_list(aspects = [check_licenses_aspect]),
    },
)
