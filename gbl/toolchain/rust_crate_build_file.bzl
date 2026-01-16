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

"""Generates BUILD file content for a rust crate."""

def rust_crate_build_file(
        name,
        rule = "rust_library",
        crate_name = "",
        deps = [],
        proc_macro_deps = [],
        features = [],
        edition = "2021",
        rustc_flags = []):
    """Generate BUILD file content for a rust crate

    This helper is suitable for crates that have straightforward build rules. Specifically, the
    crate contains a single Rust target that includes all source files under the repo.
    There is not any need of preprocessing, patching or source generation.

    Args:
        name (String): name of the rust_library target. Also used as the package name.
        rule (String): Bazel Rust rule to build, defaults to `rust_library`.
        crate_name (String): name of the rust_library crate, same as name by default.
        deps (List of strings): The `deps` field.
        proc_macro_deps (List of strings): The `proc_macro_deps` field.
        features (List of strings): The `features` field.
        edition (String): Rust edition.
        rustc_flags (List of strings): The `rustc_flags` field.

    Returns:
        A string for the BUILD file content.
    """
    crate_name = name if len(crate_name) == 0 else crate_name
    deps = "[{}]".format(",".join(['"{}"'.format(ele) for ele in deps]))
    proc_macro_deps = "[{}]".format(",".join(['"{}"'.format(ele) for ele in proc_macro_deps]))
    features = "[{}]".format(",".join(['"{}"'.format(ele) for ele in features]))
    rustc_flags = "[{}]".format(",".join(['"{}"'.format(ele) for ele in rustc_flags]))

    return """
load("@rules_rust//rust:defs.bzl", "{rule}")
load("@gbl//licensing:licenses.bzl", "generate_license")

generate_license(
    name = "license",
    package_name = "{name}",
)

{rule}(
    name = "{name}",
    crate_name = "{crate_name}",
    srcs = glob(["**/*.rs"]),
    crate_features = {features},
    edition = "{edition}",
    rustc_flags = {rustc_flags},
    visibility = ["//visibility:public"],
    deps = {deps},
    proc_macro_deps = {proc_macro_deps},
    applicable_licenses = [":license"],
)
""".format(
        name = name,
        crate_name = crate_name,
        features = features,
        rustc_flags = rustc_flags,
        deps = deps,
        proc_macro_deps = proc_macro_deps,
        edition = edition,
        rule = rule,
    )
