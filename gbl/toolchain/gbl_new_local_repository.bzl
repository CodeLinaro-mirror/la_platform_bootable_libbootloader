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

"""
This file contains the `gbl_new_local_repository` rule
"""

load(":rust_crate_build_file.bzl", "rust_crate_build_file")

def _gbl_new_local_repository_common_impl(repo_ctx, build_file, build_file_content):
    path = repo_ctx.workspace_root.get_child(repo_ctx.attr.path)
    if path.exists:
        # Symlink everything into the assembled repo.
        for entry in path.readdir():
            # Ignore native BUILD file as we'll use override from the given build_file instead
            if entry.basename == "BUILD" or entry.basename == "BUILD.bazel":
                continue
            repo_ctx.symlink(entry, repo_ctx.path(entry.basename))

    # Symlink the provided build file or use the given build file content
    if build_file != None and not build_file_content:
        repo_ctx.symlink(build_file, "BUILD")
    elif build_file == None:
        repo_ctx.file("BUILD", build_file_content)
    else:
        fail("Exactly one of build_file or build_file_content must be provided")

def _gbl_new_local_repository_impl(repo_ctx):
    _gbl_new_local_repository_common_impl(
        repo_ctx,
        repo_ctx.attr.build_file,
        repo_ctx.attr.build_file_content,
    )

gbl_new_local_repository = repository_rule(
    doc = """Assemble a new local repository with a custom top-level BUILD file

    Unlike "new_local_repository" from "@bazel_tools//tools/build_defs/repo:local.bzl", this ignores
    existing BUILD files in path.
""",
    implementation = _gbl_new_local_repository_impl,
    attrs = {
        "path": attr.string(
            mandatory = True,
            doc = "Path to the source repo, relative to the workspace root",
        ),
        "build_file": attr.label(doc = "Label of the build file to use"),
        "build_file_content": attr.string(doc = "Content of the build file to use"),
    },
)

# Hack for missing original_name. original_name is available on 8.1 and above.
# https://github.com/bazelbuild/bazel/issues/24467
def _get_original_name_legacy(repo_ctx):
    idx = repo_ctx.attr.name.rfind("+")
    return repo_ctx.attr.name[idx + 1:]

def _gbl_rust_crate_repository_impl(repo_ctx):
    target_name = getattr(repo_ctx, "original_name", _get_original_name_legacy(repo_ctx))

    _gbl_new_local_repository_common_impl(
        repo_ctx,
        None,
        rust_crate_build_file(
            target_name,
            rule = repo_ctx.attr.rule,
            crate_name = repo_ctx.attr.crate_name,
            deps = repo_ctx.attr.deps,
            proc_macro_deps = repo_ctx.attr.proc_macro_deps,
            features = repo_ctx.attr.features,
            edition = repo_ctx.attr.edition,
            rustc_flags = repo_ctx.attr.rustc_flags,
        ),
    )

gbl_rust_crate_repository = repository_rule(
    doc = """Like gbl_new_local_repository but for crates.""",
    implementation = _gbl_rust_crate_repository_impl,
    attrs = {
        "path": attr.string(
            mandatory = True,
            doc = "Path to the source repo, relative to the workspace root",
        ),
        "rule": attr.string(
            doc = "Bazel Rust rule to build.",
            default = "rust_library",
        ),
        "crate_name": attr.string(doc = "name of the rust_library crate, same as name by default."),
        "deps": attr.string_list(doc = "The `deps` field."),
        "proc_macro_deps": attr.string_list(doc = "The `proc_macro_deps` field."),
        "edition": attr.string(doc = "Rust edition.", default = "2021"),
        "rustc_flags": attr.string_list(doc = "The `rustc_flags` field."),
    },
)
