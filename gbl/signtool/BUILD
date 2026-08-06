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

load("@rules_pkg//pkg:mappings.bzl", "pkg_attributes", "pkg_files")
load("@rules_python//python:defs.bzl", "py_binary", "py_test")

package(
    default_applicable_licenses = ["@gbl//licensing:gbl_license"],
)

py_binary(
    name = "gblsigntool_py",
    srcs = ["gblsigntool.py"],
    main = "gblsigntool.py",
    visibility = ["@gbl//efi:__subpackages__"],
    deps = ["@avb//:libavbtool"],
)

filegroup(
    name = "gblsigntool_zip",
    srcs = [":gblsigntool_py"],
    output_group = "python_zip_file",
)

genrule(
    name = "gblsigntool_executable",
    srcs = [":gblsigntool_zip"],
    outs = ["gblsigntool"],
    cmd = """
cat -<<'PRELUDE' $(SRCS) > $(OUTS)
#!/bin/bash
unset PYTHONPATH RUNFILES_DIR RUNFILES_MANIFEST_FILE
exec python3 "$$0" "$$@"
PRELUDE
    """,
    executable = True,
    visibility = ["//visibility:public"],
)

py_test(
    name = "gblsigntool_test",
    srcs = ["gblsigntool_test.py"],
    data = [
        ":gblsigntool_executable",
        "//signtool/testdata",
    ],
    visibility = ["//visibility:public"],
)

pkg_files(
    name = "gblsigntool_dist_files",
    srcs = [":gblsigntool_executable"],
    attributes = pkg_attributes(mode = "0755"),
    visibility = ["//efi:__pkg__"],
)
