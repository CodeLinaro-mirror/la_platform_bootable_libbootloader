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

load("@bazel_skylib//rules:common_settings.bzl", "bool_setting")
load(
    "@gbl_llvm_prebuilts//:info.bzl",
    "LINUX_SYSROOT_INCLUDES",
    "LLVM_PREBUILTS_CPP_INCLUDE",
    "LLVM_PREBUILTS_C_INCLUDE",
    "gbl_llvm_tool_path",
)
load("@rules_rust//bindgen:defs.bzl", "rust_bindgen_toolchain")
load(
    "@rules_rust//rust:toolchain.bzl",
    "rust_stdlib_filegroup",
    "rust_toolchain",
    "rustfmt_toolchain",
)
load(
    ":gbl_toolchain.bzl",
    "build_with_no_rust_sysroot",
    "gbl_clang_cc_toolchain",
    "prebuilt_binary",
)

package(
    default_visibility = ["//visibility:public"],
)

# The following constraint/settings will be used in our newly defined
# toolchains, so that they only get selected by bazel when we configure
# to build GBL.
constraint_setting(name = "firmware_image_type")

constraint_value(
    name = "uefi",
    constraint_setting = ":firmware_image_type",
)

constraint_value(
    name = "elf",
    constraint_setting = ":firmware_image_type",
)

# This constraint specifies whether we're building for a dev or prod board.
constraint_setting(
    name = "board_class",
    # Default to prod for now; eventually we may want to update all call sites
    # and force the caller to explicitly declare which variant to build.
    default_constraint_value = ":prod",
)

constraint_value(
    name = "dev",
    constraint_setting = ":board_class",
)

constraint_value(
    name = "prod",
    constraint_setting = ":board_class",
)

# Rustc flags to add based on the dev/prod board class.
# This is intended to be used inside a `select()` statement.
rustc_dev_prod_flags_select = {
    # Enable the "gbl_dev" feature for all dev targets.
    ":dev": [
        "--cfg",
        "feature=\"gbl_dev\"",
    ],
    # Prod is the absence of the dev feature.
    # Rust features should be additive; in this case we consider prod to be
    # the default featureset, and adding "dev" on top enables additional
    # dev-only functionality e.g. extra logging, fastboot over TCP, etc.
    ":prod": [],
}

host_x86_64_constraint_values = [
    "@platforms//os:linux",
    "@platforms//cpu:x86_64",
]

uefi_x86_64_constraint_values = [
    ":uefi",
    "@platforms//os:none",
    "@platforms//cpu:x86_64",
]

uefi_x86_32_constraint_values = [
    ":uefi",
    "@platforms//os:none",
    "@platforms//cpu:x86_32",
]

uefi_aarch64_constraint_values = [
    ":uefi",
    "@platforms//os:none",
    "@platforms//cpu:aarch64",
]

elf_riscv64_constraint_values = [
    ":elf",
    "@platforms//os:none",
    "@platforms//cpu:riscv64",
]

config_setting(
    name = "gbl_rust_host_x86_64",
    constraint_values = host_x86_64_constraint_values,
)

config_setting(
    name = "gbl_rust_uefi_x86_64",
    constraint_values = uefi_x86_64_constraint_values,
)

config_setting(
    name = "gbl_rust_uefi_x86_32",
    constraint_values = uefi_x86_32_constraint_values,
)

config_setting(
    name = "gbl_rust_uefi_aarch64",
    constraint_values = uefi_aarch64_constraint_values,
)

config_setting(
    name = "gbl_rust_elf_riscv64",
    constraint_values = elf_riscv64_constraint_values,
)

platform(
    name = "host_x86_64_dev",
    constraint_values = host_x86_64_constraint_values + [":dev"],
)

platform(
    name = "host_x86_64_prod",
    constraint_values = host_x86_64_constraint_values + [":prod"],
)

platform(
    name = "gbl_uefi_x86_64_dev",
    constraint_values = uefi_x86_64_constraint_values + [":dev"],
)

platform(
    name = "gbl_uefi_x86_64_prod",
    constraint_values = uefi_x86_64_constraint_values + [":prod"],
)

platform(
    name = "gbl_uefi_x86_32_dev",
    constraint_values = uefi_x86_32_constraint_values + [":dev"],
)

platform(
    name = "gbl_uefi_x86_32_prod",
    constraint_values = uefi_x86_32_constraint_values + [":prod"],
)

platform(
    name = "gbl_uefi_aarch64_dev",
    constraint_values = uefi_aarch64_constraint_values + [":dev"],
)

platform(
    name = "gbl_uefi_aarch64_prod",
    constraint_values = uefi_aarch64_constraint_values + [":prod"],
)

platform(
    name = "gbl_elf_riscv64_dev",
    constraint_values = elf_riscv64_constraint_values + [":dev"],
)

platform(
    name = "gbl_elf_riscv64_prod",
    constraint_values = elf_riscv64_constraint_values + [":prod"],
)

bool_setting(
    name = "rust_no_sysroot",
    build_setting_default = False,
    visibility = ["//visibility:public"],
)

config_setting(
    name = "rust_no_sysroot_true",
    flag_values = {":rust_no_sysroot": "True"},
)

# Linux host LLVM toolchain.
# The toolchain is mainly used for building/linking host tools, i.e. bindgen pre-processing
gbl_clang_cc_toolchain(
    name = "x86_64-unknown-linux-gnu",
    # Host LLVM toolchain has additional C++ stdlib.
    builtin_includes = [
        LLVM_PREBUILTS_CPP_INCLUDE,  # C++ headers must come first.
        LLVM_PREBUILTS_C_INCLUDE,
    ] + LINUX_SYSROOT_INCLUDES,
    ld_flags = [
        "-stdlib=libc++",
        "-static-libstdc++",
    ],
    target_cpu = "x86_64",
    target_system_triple = "x86_64-unknown-linux-gnu",
)

toolchain(
    name = "x86_64-unknown-linux-gnu_toolchain",
    exec_compatible_with = ["@platforms//os:linux"],
    target_compatible_with = [
        "@platforms//os:linux",
        "@platforms//cpu:x86_64",
    ],
    toolchain = ":x86_64-unknown-linux-gnu",
    toolchain_type = "@bazel_tools//tools/cpp:toolchain_type",
)

# The following defines LLVM C++ toolchains for UEFI/ELF targets. They are mostly used for the
# final step of linking Rust executables, but may also be used for compiling mixed C/C++ code.

cc_flags_common = [
    "-ffreestanding",
    "-fno-common",
    "-fno-exceptions",
    "-fno-rtti",
    # Enable optimization otherwise software AVB hash will be too slow.
    "-O3",
]

# x86_64 UEFI targets
gbl_clang_cc_toolchain(
    name = "x86_64_uefi_clang_cc_toolchain",
    cc_flags = cc_flags_common + [
        # Adding this prevents the compiler from generating mmx, sse instructions such as
        # "movsd (%esp),%xmm0" which likely isn't enabled during the bootloader stage and causes
        # crash as a result.
        "-mgeneral-regs-only",
    ],
    target_cpu = "x86_64",
    target_system_triple = "x86_64-unknown-windows-msvc",
)

toolchain(
    name = "x86_64_uefi_clang",
    exec_compatible_with = ["@platforms//os:linux"],
    target_compatible_with = uefi_x86_64_constraint_values,
    toolchain = ":x86_64_uefi_clang_cc_toolchain",
    toolchain_type = "@bazel_tools//tools/cpp:toolchain_type",
)

# x86_32 UEFI targets
gbl_clang_cc_toolchain(
    name = "x86_32_uefi_clang_cc_toolchain",
    cc_flags = cc_flags_common + [
        "-m32",
        # Adding this prevents the compiler from generating mmx, sse instructions such as
        # "movsd (%esp),%xmm0" which likely isn't enabled during the bootloader stage and causes
        # crash as a result.
        "-march=i686",
    ],
    # Safe Exception Handlers is not applicable to EFI systems.
    ld_flags = ["/SAFESEH:no"],
    target_cpu = "x86_32",
    target_system_triple = "i686-unknown-windows-gnu",
)

toolchain(
    name = "x86_32_uefi_clang",
    exec_compatible_with = ["@platforms//os:linux"],
    target_compatible_with = uefi_x86_32_constraint_values,
    toolchain = ":x86_32_uefi_clang_cc_toolchain",
    toolchain_type = "@bazel_tools//tools/cpp:toolchain_type",
)

# aarch64 UEFI targets
gbl_clang_cc_toolchain(
    name = "aarch64_uefi_clang_cc_toolchain",
    cc_flags = cc_flags_common,
    target_cpu = "aarch64",
    target_system_triple = "aarch64-windows-msvc",
)

toolchain(
    name = "aarch64_uefi_clang",
    exec_compatible_with = ["@platforms//os:linux"],
    target_compatible_with = uefi_aarch64_constraint_values,
    toolchain = ":aarch64_uefi_clang_cc_toolchain",
    toolchain_type = "@bazel_tools//tools/cpp:toolchain_type",
)

# riscv64 ELF targets
gbl_clang_cc_toolchain(
    name = "riscv64_elf_clang_cc_toolchain",
    cc_flags = cc_flags_common + ["-fpie"],
    target_cpu = "riscv64",
    target_system_triple = "riscv64-unknown-linux",
)

toolchain(
    name = "riscv64_elf_clang",
    exec_compatible_with = ["@platforms//os:linux"],
    target_compatible_with = elf_riscv64_constraint_values,
    toolchain = ":riscv64_elf_clang_cc_toolchain",
    toolchain_type = "@bazel_tools//tools/cpp:toolchain_type",
)

# Rust toolchains

# An empty rust std file group used as placeholder when building rust sysroot.
rust_stdlib_filegroup(
    name = "rust_stdlib_empty",
    srcs = [],
)

# A rule for building sysroot from source.
build_with_no_rust_sysroot(
    name = "rust_sysroot_source_build",
    deps = [
        "@rust_prebuilts//:liballoc",
        "@rust_prebuilts//:libcompiler_builtins",
        "@rust_prebuilts//:libcore",
    ],
)

rust_stdlib_filegroup(
    name = "rust_std_source_build",
    srcs = [":rust_sysroot_source_build"],
)

common_lint_opts = [
    "-A",
    "deprecated",
    # external/rust/crates/spin doesn't pass this lint check and there is not a way to make an
    # exemption. Disable it until upstream is fixed or we figure out a workaround.
    #"-D",
    #"unsafe_op_in_unsafe_fn",
    "-D",
    "warnings",
    "-D",
    "clippy::undocumented_unsafe_blocks",
    "-D",
    "clippy::too-many-arguments",
]

# Linux x86_64 Host toolchain
rust_toolchain(
    name = "x86_64_unknown_linux_gnu",
    allocator_library = None,
    binary_ext = "",
    default_edition = "2021",
    dylib_ext = ".so",
    exec_triple = "x86_64-unknown-linux-gnu",
    extra_rustc_flags = select(rustc_dev_prod_flags_select),
    global_allocator_library = None,
    rust_doc = "@rust_prebuilts//:bin/rustdoc",
    rust_std = "@rust_prebuilts//:prebuilt_stdlibs",
    rustc = "@rust_prebuilts//:bin/rustc",
    staticlib_ext = ".a",
    stdlib_linkflags = [],
    target_triple = "x86_64-unknown-linux-gnu",
)

toolchain(
    name = "x86_64_unknown_linux_gnu_toolchain",
    exec_compatible_with = ["@platforms//os:linux"],
    target_settings = [":gbl_rust_host_x86_64"],
    toolchain = ":x86_64_unknown_linux_gnu",
    toolchain_type = "@rules_rust//rust:toolchain",
)

# Common flags for all Rust UEFI toolchains.
# Note this disables the default sysroot, so the rule must explicitly set `rust_std`.
uefi_rustc_flags = [
    # The linker options generated by rustc assumes `lld` as the linker. For windows platforms,
    # the lld args style is different than that of clang++, i.e. for library search path, lld
    # uses "/L<path>" while clang++ uses "-L<path>". Thus we need to use lld directly instead
    # of `clang++`.
    "--codegen=linker={}".format(gbl_llvm_tool_path("lld")),
    # Disable default sysroot. We'll pass sysroot explicitly via the `rust_std` field.
    "--sysroot",
    "/dev/null",
    # The bootloader does not persist for long and does not directly
    # do any heavy computation.
    # Trade some performance for additional security checks.
    "-C",
    "overflow-checks=true",
    # Stack protector must be one of {none,all}
    # Note: because the UEFI toolchain does not link against a libc
    # we need to define the required symbols manually.
    # All required functions must be implemented in C to prevent
    # infinite recursion.
    "-Z",
    "stack-protector=all",
]

# x86_64 UEFI toolchain
rust_toolchain(
    name = "x86_64-unknown-uefi",
    allocator_library = None,
    binary_ext = ".efi",
    default_edition = "2021",
    dylib_ext = ".so",
    exec_triple = "x86_64-unknown-linux-gnu",
    extra_rustc_flags = [
        "-Z",
        "cf-protection=full",
    ] + common_lint_opts + uefi_rustc_flags + select(rustc_dev_prod_flags_select),
    global_allocator_library = None,
    rust_doc = "@rust_prebuilts//:bin/rustdoc",
    rust_std = "@rust_prebuilts//:x86_64-unknown-uefi_prebuilt_stdlibs",
    rustc = "@rust_prebuilts//:bin/rustc",
    staticlib_ext = ".a",
    stdlib_linkflags = [],
    target_triple = "x86_64-unknown-uefi",
)

toolchain(
    name = "x86_64-unknown-uefi_toolchain",
    exec_compatible_with = ["@platforms//os:linux"],
    target_settings = [":gbl_rust_uefi_x86_64"],
    toolchain = ":x86_64-unknown-uefi",
    toolchain_type = "@rules_rust//rust:toolchain",
)

# i686 UEFI toolchain
rust_toolchain(
    name = "i686-unknown-uefi",
    allocator_library = None,
    binary_ext = ".efi",
    default_edition = "2021",
    dylib_ext = ".so",
    exec_triple = "x86_64-unknown-linux-gnu",
    extra_rustc_flags = common_lint_opts + uefi_rustc_flags + select(rustc_dev_prod_flags_select),
    global_allocator_library = None,
    rust_doc = "@rust_prebuilts//:bin/rustdoc",
    rust_std = "@rust_prebuilts//:i686-unknown-uefi_prebuilt_stdlibs",
    rustc = "@rust_prebuilts//:bin/rustc",
    staticlib_ext = ".a",
    stdlib_linkflags = [],
    target_triple = "i686-unknown-uefi",
)

toolchain(
    name = "i686-unknown-uefi_toolchain",
    exec_compatible_with = ["@platforms//os:linux"],
    target_settings = [":gbl_rust_uefi_x86_32"],
    toolchain = ":i686-unknown-uefi",
    toolchain_type = "@rules_rust//rust:toolchain",
)

# aarch64 UEFI toolchain
rust_toolchain(
    name = "aarch64-unknown-uefi",
    allocator_library = None,
    binary_ext = ".efi",
    default_edition = "2021",
    dylib_ext = ".so",
    exec_triple = "x86_64-unknown-linux-gnu",
    extra_rustc_flags = [
        "-Z",
        "branch-protection=bti,pac-ret,leaf",
    ] + common_lint_opts + uefi_rustc_flags + select(rustc_dev_prod_flags_select),
    global_allocator_library = None,
    rust_doc = "@rust_prebuilts//:bin/rustdoc",
    rust_std = "@rust_prebuilts//:aarch64-unknown-uefi_prebuilt_stdlibs",
    rustc = "@rust_prebuilts//:bin/rustc",
    staticlib_ext = ".a",
    stdlib_linkflags = [],
    target_triple = "aarch64-unknown-uefi",
)

toolchain(
    name = "aarch64-unknown-uefi_toolchain",
    exec_compatible_with = ["@platforms//os:linux"],
    target_settings = [":gbl_rust_uefi_aarch64"],
    toolchain = ":aarch64-unknown-uefi",
    toolchain_type = "@rules_rust//rust:toolchain",
)

# riscv64 PIE ELF toolchain
rust_toolchain(
    name = "riscv64gc-unknown-none-pie_elf",
    allocator_library = None,
    binary_ext = "",
    default_edition = "2021",
    dylib_ext = ".so",
    exec_triple = "x86_64-unknown-linux-gnu",
    extra_rustc_flags = common_lint_opts + uefi_rustc_flags + select(rustc_dev_prod_flags_select),
    global_allocator_library = None,
    rust_doc = "@rust_prebuilts//:bin/rustdoc",
    # Need to use our own built sysroot because we are using a custom spec below.
    rust_std = select({
        ":rust_no_sysroot_true": ":rust_stdlib_empty",
        "//conditions:default": ":rust_std_source_build",
    }),
    rustc = "@rust_prebuilts//:bin/rustc",
    staticlib_ext = ".a",
    stdlib_linkflags = [],
    # The custom spec is based on builtin target "riscv64gc-unknown-none-elf" but with the
    # following changes for enabling PIE:
    #
    #   1. "relocation-model" config entry changed from "static" to "pic".
    #   2. Added "position-independent-executables": true.
    #
    # The original spec can be obtained by:
    #
    #   rustc +nightly -Z unstable-options --print target-spec-json \
    #     --target riscv64gc-unknown-none-elf
    target_json = """
                {
                    "arch": "riscv64",
                    "code-model": "medium",
                    "cpu": "generic-rv64",
                    "data-layout": "e-m:e-p:64:64-i64:64-i128:128-n32:64-S128",
                    "eh-frame-header": false,
                    "emit-debug-gdb-scripts": false,
                    "features": "+m,+a,+f,+d,+c",
                    "is-builtin": false,
                    "linker": "rust-lld",
                    "linker-flavor": "ld.lld",
                    "llvm-abiname": "lp64d",
                    "llvm-target": "riscv64-unknown-linux",
                    "max-atomic-width": 64,
                    "os": "none",
                    "panic-strategy": "abort",
                    "relocation-model": "pic",
                    "position-independent-executables": true,
                    "supported-sanitizers": [
                        "kernel-address"
                    ],
                    "target-pointer-width": "64"
                }
""",
)

toolchain(
    name = "riscv64gc-unknown-none-pie_elf_toolchain",
    exec_compatible_with = ["@platforms//os:linux"],
    target_settings = ["@gbl//toolchain:gbl_rust_elf_riscv64"],
    toolchain = ":riscv64gc-unknown-none-pie_elf",
    toolchain_type = "@rules_rust//rust:toolchain",
)

# rustfmt toolchain
rustfmt_toolchain(
    name = "rustfmt",
    rustfmt = "@rust_prebuilts//:bin/rustfmt",
)

toolchain(
    name = "rustfmt_toolchain",
    exec_compatible_with = ["@platforms//os:linux"],
    toolchain = ":rustfmt",
    toolchain_type = "@rules_rust//rust/rustfmt:toolchain_type",
)

# Rust bindgen toolchain

prebuilt_binary(
    name = "bindgen_prebuilt",
    bin = "@bindgen//:bindgen",
)

rust_bindgen_toolchain(
    name = "bindgen_toolchain_impl",
    bindgen = ":bindgen_prebuilt",
    clang = "@gbl_llvm_prebuilts//:clang-bin",
    libclang = "@gbl_llvm_prebuilts//:libclang",
    libstdcxx = "@gbl_llvm_prebuilts//:libc++",
)

toolchain(
    name = "bindgen_toolchain",
    toolchain = "bindgen_toolchain_impl",
    toolchain_type = "@rules_rust//bindgen:toolchain_type",
)
