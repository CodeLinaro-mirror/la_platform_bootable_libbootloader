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

load("@bazel_skylib//rules:write_file.bzl", "write_file")
load("@rules_python//python:defs.bzl", "py_binary", "py_library")
load("//tools/gpt_gen:gpt_gen.bzl", "gpt_gen")

package(
    default_applicable_licenses = ["@gbl//licensing:gbl_license"],
    default_visibility = ["//visibility:public"],
)

py_library(
    name = "qemu_test_utils",
    srcs = ["qemu_test_utils.py"],
)

py_binary(
    name = "fastboot_reboot_test",
    srcs = ["fastboot_reboot_test.py"],
    imports = ["."],
    deps = [":qemu_test_utils"],
)

genrule(
    name = "test_boot_img",
    srcs = [
        "@gbl//tests/qemu/test_kernel:test_kernel_aarch64",
        "@gbl//libgbl/testdata:android/dtb_a.img",
    ],
    outs = ["boot_a.img"],
    cmd = """
        $(location @mkbootimg//:mkbootimg) \
        --kernel $(location @gbl//tests/qemu/test_kernel:test_kernel_aarch64) \
        --header_version 2 \
        --dtb $(location @gbl//libgbl/testdata:android/dtb_a.img) \
        -o $@
    """,
    tools = ["@mkbootimg"],
)

gpt_gen(
    name = "test_gpt_disk",
    disk_size = "10M",
    partitions = {
        "boot_a": {
            "size": "1M",
            "file": ":test_boot_img",
        },
        "boot_b": {"size": "1M"},
        "misc": {
            "size": "1M",
            "file": ":misc_boot_bootloader",
        },
    },
)

write_file(
    name = "misc_boot_bootloader",
    out = "misc_boot_bootloader.img",
    content = ["bootonce-bootloader\0"],
)

genrule(
    name = "qemu_test",
    srcs = [
        "qemu_launcher.py",
        "@gbl//tests/qemu/gbl_launcher:gbl_launcher_aarch64",
        "@gbl//efi:aarch64_dev",
        "@vmm//:qemu/x86_64-linux-gnu/bin/gbl-qemu-system-aarch64",
        "@vmm//:qemu/x86_64-linux-gnu/usr/share/qemu/edk2-aarch64-code.fd",
        ":test_gpt_disk",
    ],
    outs = ["output_result.txt"],
    cmd = """
        python3 $(location qemu_launcher.py) \
            $(location @gbl//tests/qemu/gbl_launcher:gbl_launcher_aarch64) \
            $(location @gbl//efi:aarch64_dev) \
            --qemu $(location @vmm//:qemu/x86_64-linux-gnu/bin/gbl-qemu-system-aarch64) \
            --bios $(location @vmm//:qemu/x86_64-linux-gnu/usr/share/qemu/edk2-aarch64-code.fd) \
            --log_output $@ \
            --disk $(location :test_gpt_disk) \
            --vhost_device_vsock $(location @vhost_device_vsock) \
            --test_script $(location :fastboot_reboot_test)
    """,
    tools = [
        ":fastboot_reboot_test",
        "@gbl//tools/gpt_gen",
        "@vhost_device_vsock",
    ],
)
