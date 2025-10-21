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

"""
This file contains rules and logic for setting up GBL workspace dependencies in the AOSP
u-boot-mainline branch.
"""

load("@gbl//toolchain:gbl_new_local_repository.bzl", "gbl_new_local_repository", "gbl_rust_crate_repository")
load("@gbl//toolchain:gbl_workspace_util.bzl", "GBL_RUST_VERSION", "gbl_config", "gbl_llvm_prebuilts")

_CLANG_VERSION = "r547379"

def define_gbl_workspace(name = None):
    """Set up worksapce dependencies for GBL

    Dependencies are checked out during "repo init". The rule simply maps them to the correct repo
    names.

    Args:
        name (String): Placeholder for buildifier check.
    """

    gbl_new_local_repository(
        name = "llvm_linux_x86_64_prebuilts",
        path = "prebuilts/clang/host/linux-x86/clang-{}".format(_CLANG_VERSION),
        build_file_content = "",
    )

    gbl_new_local_repository(
        name = "linux_x86_64_sysroot",
        path = "prebuilts/gcc/linux-x86/host/x86_64-linux-glibc2.17-4.8",
        build_file_content = """
load("@rules_cc//cc:defs.bzl", "cc_library")
exports_files(glob(["**/*"]))
cc_library(
    name = "linux_x86_64_sysroot_include",
    hdrs = glob(["sysroot/usr/include/**/*.h"]),
    includes = [ "sysroot/usr/include", "sysroot/usr/include/x86_64-linux-gnu" ],
    visibility = ["//visibility:public"],
)
""",
    )

    gbl_new_local_repository(
        name = "rust_prebuilts",
        path = "prebuilts/rust/linux-x86/{}".format(GBL_RUST_VERSION),
        build_file = "@gbl//toolchain:BUILD.android_rust_prebuilts.bazel",
    )

    gbl_new_local_repository(
        name = "bindgen",
        path = "prebuilts/clang-tools/linux-x86/bin",
        build_file_content = """exports_files(["bindgen"])""",
    )

    gbl_new_local_repository(
        name = "elfutils",
        path = "external/elfutils",
        build_file_content = """
load("@rules_cc//cc:defs.bzl", "cc_library")
cc_library(
    name = "elf_type_header",
    hdrs = ["libelf/elf.h"],
    visibility = ["//visibility:public"],
)
""",
    )

    gbl_new_local_repository(
        name = "mkbootimg",
        path = "tools/mkbootimg",
        build_file_content = """
load("@rules_cc//cc:defs.bzl", "cc_library")
exports_files(glob(["**/*"]))
cc_library(
    name = "bootimg_header",
    hdrs = ["include/bootimg/bootimg.h"],
    includes = ["include"],
    visibility = ["//visibility:public"],
)
""",
    )

    gbl_new_local_repository(
        name = "libfdt_c",
        path = "external/dtc/libfdt",
        build_file = "@gbl//libfdt:BUILD.libfdt_c.bazel",
    )

    gbl_new_local_repository(
        name = "libufdt_c",
        path = "external/libufdt",
        build_file = "@gbl//libfdt:BUILD.libufdt_c.bazel",
    )

    gbl_new_local_repository(
        name = "libdttable_c",
        path = "external/libufdt/utils/src",
        build_file = "@gbl//libdttable:BUILD.libdttable_c.bazel",
    )

    gbl_new_local_repository(
        name = "arm_trusted_firmware",
        path = "external/arm-trusted-firmware",
        build_file = "@gbl//libboot/aarch64_cache_helper:BUILD.arm_trusted_firmware.bazel",
    )

    gbl_new_local_repository(
        name = "avb",
        path = "external/avb",
        build_file = "@gbl//libavb:BUILD.avb.bazel",
    )

    gbl_rust_crate_repository(
        name = "uuid",
        path = "external/rust/android-crates-io/crates/uuid",
    )

    gbl_rust_crate_repository(
        name = "spin",
        path = "external/rust/android-crates-io/crates/spin",
        features = [
            "mutex",
            "spin_mutex",
        ],
        rustc_flags = [
            "-A",
            "unused_imports",
        ],
    )

    gbl_rust_crate_repository(
        name = "static_assertions",
        path = "external/rust/android-crates-io/crates/static_assertions",
    )

    gbl_rust_crate_repository(
        name = "managed",
        path = "external/rust/android-crates-io/crates/managed",
        features = ["map"],
        rustc_flags = [
            "-A",
            "unused_macros",
            "-A",
            "redundant_semicolons",
        ],
    )

    gbl_rust_crate_repository(
        name = "itertools",
        path = "external/rust/android-crates-io/crates/itertools",
        deps = ["@either"],
        features = ["default", "use_std", "use_alloc"],
        rustc_flags = ["-A", "dead_code"],
    )

    gbl_rust_crate_repository(
        name = "itertools_noalloc",
        path = "external/rust/android-crates-io/crates/itertools",
        crate_name = "itertools",
        features = [],
        deps = ["@either_noalloc"],
        rustc_flags = ["-A", "dead_code"],
    )

    gbl_rust_crate_repository(
        name = "either",
        path = "external/rust/android-crates-io/crates/either",
        features = ["default", "use_std"],
    )

    gbl_rust_crate_repository(
        name = "either_noalloc",
        path = "external/rust/android-crates-io/crates/either",
        crate_name = "either",
        features = [],
    )

    # TODO(b/383783832): migrate to android-crates-io
    gbl_new_local_repository(
        name = "smoltcp",
        path = "external/rust/crates/smoltcp",
        build_file = "@gbl//smoltcp:BUILD.smoltcp.bazel",
    )

    gbl_rust_crate_repository(
        name = "arrayvec",
        path = "external/rust/android-crates-io/crates/arrayvec",
        rustc_flags = ["-A", "dead_code"],
    )

    gbl_rust_crate_repository(
        name = "downcast",
        path = "external/rust/android-crates-io/crates/downcast",
        features = ["default", "std"],
    )

    gbl_rust_crate_repository(
        name = "fragile",
        path = "external/rust/android-crates-io/crates/fragile",
    )

    gbl_rust_crate_repository(
        name = "lazy_static",
        path = "external/rust/android-crates-io/crates/lazy_static",
    )

    gbl_rust_crate_repository(
        name = "mockall",
        path = "external/rust/android-crates-io/crates/mockall",
        deps = [
            "@cfg_if",
            "@downcast",
            "@fragile",
            "@lazy_static",
            "@predicates",
            "@predicates_tree",
        ],
        proc_macro_deps = ["@mockall_derive"],
    )

    gbl_rust_crate_repository(
        name = "mockall_derive",
        path = "external/rust/android-crates-io/crates/mockall_derive",
        rule = "rust_proc_macro",
        deps = ["@cfg_if", "@proc_macro2", "@quote", "@syn"],
    )

    gbl_rust_crate_repository(
        name = "predicates",
        path = "external/rust/android-crates-io/crates/predicates",
        deps = ["@itertools", "@predicates_core", "@termcolor"],
    )

    gbl_rust_crate_repository(
        name = "predicates_core",
        path = "external/rust/android-crates-io/crates/predicates-core",
    )

    gbl_rust_crate_repository(
        name = "predicates_tree",
        path = "external/rust/android-crates-io/crates/predicates-tree",
        deps = ["@predicates_core", "@termtree"],
    )

    gbl_rust_crate_repository(
        name = "termcolor",
        path = "external/rust/android-crates-io/crates/termcolor",
    )

    gbl_rust_crate_repository(
        name = "termtree",
        path = "external/rust/android-crates-io/crates/termtree",
    )

    # TODO(b/383783832): migrate to android-crates-io
    gbl_rust_crate_repository(
        name = "zune_inflate",
        path = "external/rust/crates/zune-inflate",
        features = ["gzip"],
    )

    gbl_rust_crate_repository(
        name = "lz4_flex",
        path = "external/rust/android-crates-io/crates/lz4_flex",
        features = ["safe-decode"],
        rustc_flags = ["-A", "dead_code"],
    )

    gbl_new_local_repository(
        name = "zbi",
        path = "prebuilts/fuchsia_sdk/",
        # TODO: b/413506174 - clean up ref to outer module
        # buildifier: disable=canonical-repository
        build_file = "@@//prebuilts/fuchsia_sdk:BUILD.zbi.bazel",
    )

    gbl_rust_crate_repository(
        name = "zerocopy",
        path = "external/rust/android-crates-io/crates/zerocopy",
        features = ["derive", "simd", "zerocopy-derive"],
        proc_macro_deps = ["@zerocopy_derive"],
    )

    gbl_rust_crate_repository(
        name = "zerocopy_derive",
        path = "external/rust/android-crates-io/crates/zerocopy-derive",
        rule = "rust_proc_macro",
        deps = ["@proc_macro2", "@quote", "@syn"],
    )

    gbl_rust_crate_repository(
        name = "zeroize",
        path = "external/rust/android-crates-io/crates/zeroize",
        rustc_flags = ["--cap-lints=allow"],
    )

    gbl_rust_crate_repository(
        name = "bitflags",
        path = "external/rust/android-crates-io/crates/bitflags",
    )

    gbl_rust_crate_repository(
        name = "flagset",
        path = "external/rust/android-crates-io/crates/flagset",
    )

    gbl_rust_crate_repository(
        name = "byteorder",
        path = "external/rust/android-crates-io/crates/byteorder",
    )

    gbl_rust_crate_repository(
        name = "cfg_if",
        path = "external/rust/android-crates-io/crates/cfg-if",
    )

    gbl_rust_crate_repository(
        name = "crc32fast",
        path = "external/rust/android-crates-io/crates/crc32fast",
        deps = ["@cfg_if"],
        # Current version of the crate doesn't compile with newer editions.
        edition = "2015",
    )

    gbl_rust_crate_repository(
        name = "hex",
        path = "external/rust/android-crates-io/crates/hex",
        features = ["alloc", "default", "std"],
    )

    gbl_rust_crate_repository(
        name = "quote",
        path = "external/rust/android-crates-io/crates/quote",
        features = ["default", "proc-macro"],
        deps = ["@proc_macro2"],
    )

    gbl_rust_crate_repository(
        name = "unicode_ident",
        path = "external/rust/android-crates-io/crates/unicode-ident",
    )

    gbl_rust_crate_repository(
        name = "syn",
        path = "external/rust/android-crates-io/crates/syn",
        features = [
            "clone-impls",
            "default",
            "derive",
            "extra-traits",
            "full",
            "parsing",
            "printing",
            "proc-macro",
            "quote",
            "visit",
            "visit-mut",
        ],
        deps = ["@proc_macro2", "@quote", "@unicode_ident"],
    )

    gbl_rust_crate_repository(
        name = "proc_macro2",
        path = "external/rust/android-crates-io/crates/proc-macro2",
        deps = ["@unicode_ident"],
        features = ["default", "proc-macro", "span-locations"],
    )

    gbl_new_local_repository(
        name = "boringssl",
        path = "external/boringssl/src",
        build_file = "@gbl//libboringssl:BUILD.boringssl.bazel",
    )

    gbl_new_local_repository(
        name = "open_dice",
        path = "external/open-dice",
        build_file = "@gbl//libopendice:BUILD.open_dice.bazel",
    )

    gbl_rust_crate_repository(
        name = "bytes",
        path = "external/rust/android-crates-io/crates/bytes",
    )

    # Set up a repo to export LLVM tool/library/header/sysroot paths
    gbl_llvm_prebuilts(name = "gbl_llvm_prebuilts")

    # We don't register GBL toolchains here because they will be masked by toolchains from
    # `build/kleaf//:` as they are registered earlier. Instead, we will pass GBL toolchains via
    # bazel commandline argument "--extra_toolchains=@gbl//toolchain:all" when building GBL
    # targets, which allows them to be evaluated first during toolchain resolution.

    gbl_config(name = "gbl_config")

# buildifier: disable=unused-variable
def _gbl_repositories_ext_impl(module_ctx):
    define_gbl_workspace()

gbl_repositories_ext = module_extension(
    implementation = _gbl_repositories_ext_impl,
)
