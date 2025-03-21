# Copyright (C) 2023 The Android Open Source Project
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

load("@gbl//toolchain:gbl_toolchain.bzl", "build_with_platform", "dev_test")
load("@gbl//toolchain:gbl_workspace_util.bzl", "ANDROID_RUST_LINTS")
load("@gbl_llvm_prebuilts//:info.bzl", "gbl_llvm_tool_path")
load("@rules_rust//rust:defs.bzl", "rust_binary", "rust_library", "rust_test")

package(
    default_visibility = ["//visibility:public"],
)

rust_library(
    name = "libgbl_efi",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "gbl_efi",
    rustc_flags = ANDROID_RUST_LINTS,
    deps = [
        "@arrayvec",
        "@avb",
        "@gbl//libasync",
        "@gbl//libasync:cyclic_executor",
        "@gbl//libboot",
        "@gbl//libefi",
        "@gbl//libefi_types",
        "@gbl//liberror",
        "@gbl//libfastboot",
        "@gbl//libfdt",
        "@gbl//libgbl",
        "@gbl//libsafemath",
        "@gbl//libstorage",
        "@smoltcp",
        "@spin",
        "@uuid",
        "@zbi",
        "@zerocopy",
    ],
)

rust_test(
    name = "test",
    crate = ":libgbl_efi",
    data = [
        "@gbl//libfdt/test/data:all",
    ],
    # TODO(b/355436086): mock out the rest of the libefi APIs and
    # remove dead-code; for now it would require a lot of invasive
    # code changes to selectively disable things on tests so this
    # is worth it to keep things more readable.
    rustc_flags = ANDROID_RUST_LINTS + [
        "-A",
        "dead-code",
    ],
    deps = [
        "@gbl//libefi:mocks",
        "@mockall",
    ],
)

dev_test(
    name = "test_dev",
    target = ":test",
)

# The UEFI application target.
#
# Almost all logic should be in the libgbl_efi target; this contains only the
# things that would prevent unittesting such as global allocation hooks or
# target-specific compiler dependencies.
rust_binary(
    name = "app",
    srcs = glob(["app/**/*.rs"]),
    linker_script = select(
        {
            "@gbl//toolchain:gbl_rust_elf_riscv64": "@gbl//efi/arch/riscv64:riscv64_efi.lds",
            "//conditions:default": None,
        },
    ),
    rustc_flags = ANDROID_RUST_LINTS + [
        "-C",
        "panic=abort",
    ],
    deps = [
        ":libgbl_efi",
        "@avb//:avb_crypto_ops_sha_impl_staticlib",
        "@gbl//libavb:sysdeps",
        "@gbl//libefi",
        "@gbl//libefi_types",
    ] + select(
        {
            "@gbl//toolchain:gbl_rust_elf_riscv64": [
                "@gbl//efi/arch/riscv64:efi_arch_deps_riscv64",
            ],
            "//conditions:default": [],
        },
    ),
)

genrule(
    name = "gbl_efi",
    srcs = [":app"],
    outs = ["gbl.efi"],
    cmd = select(
        {
            # For RISCV target, existing toolchain can only generate ELF format image.
            # The current solution is to manually add a PE/COFF header at image
            # start and use objcopy to remove the ELF header to make it a PE/COFF image.
            # Also use `elf_static_relocation_checker` to check that our relocation library
            # can handle all the generated relocation types. The following expands to two commands:
            #
            # 1. `llvm-objcopy <elf image> -O binary <efi image>`
            # 2. `elf_static_relocation_checker <elf image> <efi image>`
            "@gbl//toolchain:gbl_rust_elf_riscv64": """
            {} -O binary $(location @gbl//efi:app) $(OUTS) && \\
            $(location @gbl//libelf:elf_static_relocation_checker) $(SRCS) $(OUTS)
        """.format(
                gbl_llvm_tool_path("llvm-objcopy"),
            ),
            "//conditions:default": "cp $(SRCS) $(OUTS)",
        },
    ),
    tools = select(
        {
            "@gbl//toolchain:gbl_rust_elf_riscv64": [
                "@gbl//libelf:elf_static_relocation_checker",
            ],
            "//conditions:default": [],
        },
    ),
)

build_with_platform(
    name = "x86_64_dev",
    platform = "@gbl//toolchain:gbl_uefi_x86_64_dev",
    deps = [":gbl_efi"],
)

build_with_platform(
    name = "x86_64_prod",
    platform = "@gbl//toolchain:gbl_uefi_x86_64_prod",
    deps = [":gbl_efi"],
)

build_with_platform(
    name = "x86_32_dev",
    platform = "@gbl//toolchain:gbl_uefi_x86_32_dev",
    deps = [":gbl_efi"],
)

build_with_platform(
    name = "x86_32_prod",
    platform = "@gbl//toolchain:gbl_uefi_x86_32_prod",
    deps = [":gbl_efi"],
)

build_with_platform(
    name = "aarch64_dev",
    platform = "@gbl//toolchain:gbl_uefi_aarch64_dev",
    deps = [":gbl_efi"],
)

build_with_platform(
    name = "aarch64_prod",
    platform = "@gbl//toolchain:gbl_uefi_aarch64_prod",
    deps = [":gbl_efi"],
)

build_with_platform(
    name = "riscv64_dev",
    platform = "@gbl//toolchain:gbl_elf_riscv64_dev",
    deps = [":gbl_efi"],
)

build_with_platform(
    name = "riscv64_prod",
    platform = "@gbl//toolchain:gbl_elf_riscv64_prod",
    deps = [":gbl_efi"],
)

# Our infra tests currently fetch GBL binaries from these paths:
# * gbl_aarch64.efi
# * gbl_riscv64.efi
# * gbl_x86_32.efi
# * gbl_x86_64.efi
# For now copy the "dev" build to these paths so the tests can find them; once
# we modify the tests to fetch a variant expicitly we can remove these copies.
genrule(
    name = "copies_for_tests",
    srcs = [
        ":aarch64_dev",
        ":riscv64_dev",
        ":x86_32_dev",
        ":x86_64_dev",
    ],
    outs = [
        "gbl_aarch64.efi",
        "gbl_riscv64.efi",
        "gbl_x86_32.efi",
        "gbl_x86_64.efi",
    ],
    # https://bazel.build/reference/be/general#general-advice says not to
    # create symlinks in genrules, so we do a full copy here. The images are
    # small enough that it's fine either way.
    cmd = """
        cp $(location :aarch64_dev) $(RULEDIR)/gbl_aarch64.efi
        cp $(location :riscv64_dev) $(RULEDIR)/gbl_riscv64.efi
        cp $(location :x86_32_dev) $(RULEDIR)/gbl_x86_32.efi
        cp $(location :x86_64_dev) $(RULEDIR)/gbl_x86_64.efi
    """,
)

filegroup(
    name = "all_platforms",
    srcs = [
        ":aarch64_dev",
        ":aarch64_prod",
        ":copies_for_tests",
        ":riscv64_dev",
        ":riscv64_prod",
        ":x86_32_dev",
        ":x86_32_prod",
        ":x86_64_dev",
        ":x86_64_prod",
    ],
)
