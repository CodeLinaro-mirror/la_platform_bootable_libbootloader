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
load("//tests/qemu:qemu_test.bzl", "qemu_test")
load("//toolchain:gbl_toolchain.bzl", "build_with_tracing")
load("//tools/gpt_gen:gpt_gen.bzl", "gpt_gen")

package(
    default_applicable_licenses = ["@gbl//licensing:gbl_license"],
    default_visibility = ["//visibility:public"],
)

py_library(
    name = "qemu_test_utils",
    srcs = ["qemu_test_utils.py"],
    data = [
        "//tools:gbl-trace-to-perfetto.py",
        "@llvm_linux_x86_64_prebuilts//:common_binaries",
    ],
    deps = ["@rules_python//python/runfiles"],
)

genrule(
    name = "test_boot_img",
    srcs = [
        "@gbl//tests/qemu/test_kernel:test_kernel_aarch64",
    ],
    outs = ["boot_a.img"],
    cmd = """
        $(location @mkbootimg//:mkbootimg) \
        --kernel $(location @gbl//tests/qemu/test_kernel:test_kernel_aarch64) \
        --header_version 4 \
        -o $@
    """,
    tools = ["@mkbootimg"],
)

gpt_gen(
    name = "test_gpt_boot",
    disk_size = "10M",
    partitions = {
        "boot_a": {
            "size": "1M",
            "file": ":test_boot_img",
        },
        "boot_b": {"size": "1M"},
        "vendor_boot_a": {
            "size": "1M",
            "file": "@gbl//libgbl/testdata:android/vendor_boot_v4_a.img",
        },
        "vendor_boot_b": {"size": "1M"},
        "init_boot_a": {
            "size": "1M",
            "file": "@gbl//libgbl/testdata:android/init_boot_a.img",
        },
        "init_boot_b": {"size": "1M"},
    },
)

write_file(
    name = "misc_boot_bootloader",
    out = "misc_boot_bootloader.img",
    content = ["bootonce-bootloader\0"],
)

gpt_gen(
    name = "test_gpt_misc_fastboot_mode",
    disk_size = "4M",
    partitions = {
        "misc": {
            "size": "1M",
            "file": ":misc_boot_bootloader",
        },
    },
)

gpt_gen(
    name = "test_gpt_misc_normal_mode",
    disk_size = "4M",
    partitions = {"misc": {"size": "1M"}},
)

py_binary(
    name = "fastboot_misc_test",
    srcs = ["fastboot_misc_test.py"],
    imports = ["."],
    deps = [":qemu_test_utils"],
)

qemu_test(
    name = "qemu_fastboot_misc_test",
    disk = [
        ":test_gpt_boot",
        ":test_gpt_misc_fastboot_mode",
    ],
    gbl = "@gbl//efi:aarch64_dev",
    test_script = ":fastboot_misc_test",
)

py_binary(
    name = "boot_memory_test",
    srcs = ["boot_memory_test.py"],
    imports = ["."],
    deps = [":qemu_test_utils"],
)

# GPT disk with empty boot_a/b partition. The image is expected to be provided via preloaded
# partition or fastboot boot command.
gpt_gen(
    name = "test_gpt_no_boot_image",
    disk_size = "10M",
    partitions = {
        "boot_a": {"size": "1M"},
        "boot_b": {"size": "1M"},
        "vendor_boot_a": {
            "size": "1M",
            "file": "@gbl//libgbl/testdata:android/vendor_boot_v4_a.img",
        },
        "vendor_boot_b": {"size": "1M"},
        "init_boot_a": {
            "size": "1M",
            "file": "@gbl//libgbl/testdata:android/init_boot_a.img",
        },
        "init_boot_b": {"size": "1M"},
    },
)

qemu_test(
    name = "qemu_boot_memory_test",
    disk = [
        ":test_gpt_no_boot_image",
        ":test_gpt_misc_normal_mode",
    ],
    env = {
        "GBL_GENERAL_LOAD_SIZE": "0x100000",  # 1MB
        "GBL_FASTBOOT_DOWNLOAD_SIZE": "0x1000000",  # 16MB
        "GBL_KERNEL_SIZE": "0x100000",  # 1MB
        "GBL_RAMDISK_SIZE": "0x100000",  # 1MB
        "GBL_FDT_SIZE": "0x100000",  # 1MB
        "GBL_PVMFW_DATA_SIZE": "0x100000",  # 1MB
        # Preloaded boot, designated vendor_boot, allocated init_boot
        "PART_PRELOADED_boot": "boot_a.img",
        "PART_DESIGNATED_vendor_boot": "0x100000",  # 1MB
    },
    gbl = ":aarch64_dev_tracing",
    runfiles = [
        ":aarch64_dev_tracing_pdb",
        ":test_boot_img",
    ],
    test_script = ":boot_memory_test",
)

build_with_tracing(
    name = "aarch64_dev_tracing",
    deps = ["@gbl//efi:aarch64_dev"],
)

build_with_tracing(
    name = "aarch64_dev_tracing_pdb",
    deps = ["@gbl//efi:aarch64_dev_pdb"],
)

py_binary(
    name = "trace_test",
    srcs = ["trace_test.py"],
    imports = ["."],
    deps = [
        ":qemu_test_utils",
    ],
)

qemu_test(
    name = "qemu_trace_test",
    disk = [
        ":test_gpt_misc_fastboot_mode",
        ":test_gpt_no_boot_image",
    ],
    gbl = ":aarch64_dev_tracing",
    runfiles = [
        ":aarch64_dev_tracing_pdb",
        ":test_boot_img",
    ],
    test_script = ":trace_test",
)
