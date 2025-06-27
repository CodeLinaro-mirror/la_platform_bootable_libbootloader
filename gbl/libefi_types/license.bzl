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
Action that verifies provided files has required SPDX identifier
"""

def _spdx_license_test_impl(ctx):
    shell_script = """
set -e

while [[ $# -gt 0 ]]; do
  case $1 in
    --in)
      INPUT=$2
      shift
      shift
      ;;
    --out)
      OUTPUT=$2
      shift
      shift
      ;;
    --expected-spdx)
      EXPECTED_SPDX=$2
      shift
      shift
      ;;
    *)
      echo "Unexpected argument: $1"
      exit 1
      ;;
  esac
done

ALL_INPUTS=$(echo ${INPUT} | sed 's/,/ /g')

FILES_SPDX_MISSING=""
for file in ${ALL_INPUTS[@]}; do
  grep -qE "${EXPECTED_SPDX}" "${file}" || FILES_SPDX_MISSING+="\n\t${file}"
done

if [ -n "${FILES_SPDX_MISSING}" ]; then
  echo -e "ERROR: The following files are missing the required SPDX header:${FILES_SPDX_MISSING}"
  exit 1
fi

touch ${OUTPUT}
"""

    input = ctx.files.srcs
    output = ctx.actions.declare_file("{name}.script".format(
        name = ctx.attr.name,
    ))
    expected_license = "SPDX-License-Identifier: {identifier}".format(
        identifier = ctx.attr.expected_spdx_identifier,
    )

    args = ctx.actions.args()
    args.add_joined(
        "--in",
        input,
        join_with = ",",
    )
    args.add(
        "--out",
        output,
    )
    args.add(
        "--expected-spdx",
        expected_license,
    )

    ctx.actions.run_shell(
        inputs = input,
        outputs = [output],
        arguments = [args],
        command = shell_script,
        mnemonic = "SpdxLicenseTest",
    )

    return [DefaultInfo(executable = output)]

spdx_license_test = rule(
    implementation = _spdx_license_test_impl,
    attrs = {
        "srcs": attr.label_list(
            doc = "Sources to check for the SPDX license.",
            allow_files = True,
            mandatory = True,
        ),
        "expected_spdx_identifier": attr.string(
            doc = "Expected SPDX-License-Identifier value to check.",
            mandatory = True,
        ),
    },
    test = True,
)
