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

load("//tools/gpt_gen:gpt_gen.bzl", "gpt_gen")

package(
    default_applicable_licenses = ["@gbl//licensing:gbl_license"],
    default_visibility = ["//visibility:public"],
)

gpt_gen(
    name = "test_gpt_disk",
    disk_size = "10M",
    partitions = {
        "boot": {"size": "1M"},
        "system": {"size": "2M"},
    },
)

genrule(
    name = "qemu_test",
    srcs = [
        "qemu_launcher.py",
        "@gbl//tests/qemu/gbl_launcher:gbl_launcher_aarch64",
        "@vmm//:qemu/x86_64-linux-gnu/bin/gbl-qemu-system-aarch64",
        "@vmm//:qemu/x86_64-linux-gnu/usr/share/qemu/edk2-aarch64-code.fd",
        ":test_gpt_disk",
    ],
    outs = ["output_result.txt"],
    cmd = """
        python3 $(location qemu_launcher.py) \
            $(location @gbl//tests/qemu/gbl_launcher:gbl_launcher_aarch64) \
            --qemu $(location @vmm//:qemu/x86_64-linux-gnu/bin/gbl-qemu-system-aarch64) \
            --bios $(location @vmm//:qemu/x86_64-linux-gnu/usr/share/qemu/edk2-aarch64-code.fd) \
            --log_output $@ \
            --disk $(location :test_gpt_disk)
    """,
    tools = ["@gbl//tools/gpt_gen"],
)
