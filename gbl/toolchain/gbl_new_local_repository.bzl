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

def _gbl_new_local_repository_impl(repo_ctx):
    path = repo_ctx.workspace_root.get_child(repo_ctx.attr.path)
    if path.exists:
        # Symlink everything into the assembled repo.
        for entry in path.readdir():
            # Ignore native BUILD file as we'll use override from `ctx.attr.build_file` instead.
            if entry.basename == "BUILD" or entry.basename == "BUILD.bazel":
                continue
            repo_ctx.symlink(entry, repo_ctx.path(entry.basename))

    # Symlink the provided build file or use the given build file content
    if repo_ctx.attr.build_file != None and not repo_ctx.attr.build_file_content:
        repo_ctx.symlink(repo_ctx.attr.build_file, "BUILD")
    elif repo_ctx.attr.build_file == None:
        repo_ctx.file("BUILD", repo_ctx.attr.build_file_content)
    else:
        fail("Exactly one of build_file or build_file_content must be provided")

gbl_new_local_repository = repository_rule(
    doc = """Assemble a new local repository with a custom top-level BUILD file

    Unlike "new_local_repository" from "@bazel//tools/build_defs/repo:local.bzl", this ignores
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
