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
This file defines `build_and_run_tests` rule
"""

load("@rules_shell//shell:sh_test.bzl", "sh_test")

TestArgsInfo = provider(
    doc = "Propagates test args from target to build_and_run",
    fields = {
        "args": "List of string arguments",
    },
)

def _test_args_aspect_impl(_target, ctx):
    args = []
    if hasattr(ctx.rule.attr, "args"):
        args = ctx.rule.attr.args
    return [TestArgsInfo(args = args)]

test_args_aspect = aspect(
    implementation = _test_args_aspect_impl,
    attr_aspects = [],
)

def _build_and_run_impl(ctx):
    # Executable file from the attribute.
    executable = ctx.executable.executable

    # Output log file.
    logfile = ctx.actions.declare_file("%s.txt" % ctx.attr.name)

    # Args from the target itself (extracted via aspect)
    args = []
    if TestArgsInfo in ctx.attr.executable:
        args = ctx.attr.executable[TestArgsInfo].args

    # Escape arguments for shell
    escaped_args = ["'%s'" % a.replace("'", "'\\''") for a in args]
    args_str = " ".join(escaped_args)

    ctx.actions.run_shell(
        inputs = [executable] + ctx.files.data,
        outputs = [logfile],
        env = ctx.attr.env,
        progress_message = "Running test %s" % executable.short_path,
        command = """\
        BIN="%s" && \
        OUT="%s" && \
        ($BIN %s > $OUT || \
        if [ $? == 0 ]; then
            true
        else
            echo "\n%s failed." && cat $OUT && false
        fi)
""" % (executable.path, logfile.path, args_str, executable.short_path),
    )

    return [DefaultInfo(files = depset([logfile]))]

build_and_run = rule(
    implementation = _build_and_run_impl,
    attrs = {
        "executable": attr.label(
            executable = True,
            cfg = "target",
            allow_files = True,
            mandatory = True,
            aspects = [test_args_aspect],
        ),
        "data": attr.label_list(
            allow_files = True,
            allow_empty = True,
        ),
        "env": attr.string_dict(
            allow_empty = True,
            default = {},
        ),
    },
)

# TODO(b/382503065): This is a temporary workaround due to presubmit infra not blocking on test
# failures and only on build failures. Removed once the issue is solved.
def build_and_run_tests(name, tests, data, envs = {}):
    """Create an `sh_test` target that run a set of unittests during build time.

    Args:
        name (String): name of the rust_library target.
        tests (List of strings): List of test target.
        data (List of strings): Runtime data needed by the tests.
        envs (Dict of string to dict): Environment variables for each test.
    """

    all_tests = []
    for idx, test in enumerate(tests):
        subtest_name = "{}_subtest_{}".format(name, idx)
        build_and_run(
            name = subtest_name,
            testonly = True,
            executable = test,
            data = data,
            env = envs.get(test, {}),
        )

        all_tests.append(":{}".format(subtest_name))

    sh_test(
        name = name,
        srcs = ["@gbl//tests:noop.sh"],
        data = data + all_tests,
    )
