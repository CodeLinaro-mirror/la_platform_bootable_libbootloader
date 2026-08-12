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

"""Minimal custom rule for compiling assembly files with NASM."""

load("@rules_cc//cc:defs.bzl", "cc_library")

def _nasm_objects_impl(ctx):
    outs = []
    workspace_root = ctx.label.workspace_root

    raw_includes = {}
    if workspace_root:
        raw_includes[workspace_root + "/"] = None
    raw_includes["."] = None

    for inc in ctx.attr.includes:
        if inc.startswith("/"):
            continue
        if workspace_root:
            raw_includes[workspace_root + "/" + inc + "/"] = None
        else:
            raw_includes[inc + "/"] = None

    for src in ctx.files.srcs:
        out = ctx.actions.declare_file("_obj/" + ctx.label.name + "/" + src.basename + ".obj")

        includes = dict(raw_includes)
        includes[src.dirname + "/"] = None

        args = ctx.actions.args()
        args.add("-f", "win64")
        args.add_all(includes.keys(), before_each = "-I")
        args.add_all(ctx.attr.copts)
        args.add("-o", out)
        args.add(src)

        ctx.actions.run(
            mnemonic = "NasmAssemble",
            executable = ctx.executable._nasm,
            arguments = [args],
            inputs = depset([src] + ctx.files.hdrs),
            outputs = [out],
        )
        outs.append(out)

    return [DefaultInfo(files = depset(outs))]

_nasm_objects = rule(
    implementation = _nasm_objects_impl,
    attrs = {
        "srcs": attr.label_list(
            allow_files = [".asm", ".s", ".nasm"],
            doc = "Assembly source files to compile.",
        ),
        "hdrs": attr.label_list(
            allow_files = [".inc", ".h"],
            doc = "Header/include files required during assembly.",
        ),
        "includes": attr.string_list(
            doc = "Include directories for NASM.",
        ),
        "copts": attr.string_list(
            doc = "Additional flags passed to NASM.",
        ),
        "_nasm": attr.label(
            default = "@nasm//:nasm",
            executable = True,
            cfg = "exec",
        ),
    },
)

def nasm_cc_library(
        name,
        srcs,
        hdrs = [],
        includes = [],
        copts = [],
        **kwargs):
    """Compiles NASM assembly sources and exposes them as a cc_library.

    Args:
        name: Name of the target.
        srcs: Assembly source files (.asm/.s).
        hdrs: Header/include files (.inc/.h).
        includes: List of include directory paths.
        copts: Additional compile flags for NASM.
        **kwargs: Additional arguments forwarded to cc_library.
    """
    _nasm_objects(
        name = name + "_objects",
        srcs = srcs,
        hdrs = hdrs,
        includes = includes,
        copts = copts,
        visibility = ["//visibility:private"],
    )
    cc_library(
        name = name,
        srcs = [":" + name + "_objects"],
        **kwargs
    )
