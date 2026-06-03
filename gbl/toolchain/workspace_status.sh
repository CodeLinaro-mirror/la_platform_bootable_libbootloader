#!/bin/bash
#
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

build_branch="$BUILD_BRANCH"
if [ -z "$build_branch" ]; then
  build_branch="gbl-android17"
fi

build_number="$BUILD_NUMBER"
if [ -z "$build_number" ]; then
  build_number="eng.${USER}.$(date +%Y-%m-%d)"
fi

revision=""
if command -v git >/dev/null 2>/dev/null; then
  PROJECT_DIR="$(realpath "$(dirname "${BASH_SOURCE[0]}")/../..")"

  revision=$(git -C "$PROJECT_DIR" rev-parse --verify --short=12 HEAD)

  if [ "$revision" ]; then
    dirty="$(
      { git -C "$PROJECT_DIR" --no-optional-locks status -uno --porcelain ||
        git -C "$PROJECT_DIR" diff-index --name-only HEAD
      } 2>/dev/null
    )"

    if [ "$dirty" ]; then
      revision="${revision}-dirty"
    fi
  fi
fi

echo "STABLE_BUILD_BRANCH ${build_branch}"
echo "STABLE_BUILD_NUMBER ${build_number}"
[ "$revision" ] && echo "STABLE_BUILD_REVISION ${revision}"

# Print debug messages to stderr.
( echo "BUILD_BRANCH=${build_branch}"
  echo "BUILD_NUMBER=${build_number}"
  echo "BUILD_REVISION=${revision}"
) >&2

exit 0
