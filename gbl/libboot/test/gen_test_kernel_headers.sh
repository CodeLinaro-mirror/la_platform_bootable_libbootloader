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

set -e

readonly SCRIPT_DIR=`cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd`
readonly DATA_DIR="${SCRIPT_DIR}/data"
readonly TMP_DIR=`mktemp -d`

# Pick a recent green build from:
#   https://ci.android.com/builds/branches/aosp_kernel-common-android-mainline
readonly BUILD_ID=15434119
readonly BASE="https://ci.android.com/builds/submitted/${BUILD_ID}"
readonly UA="Mozilla/5.0"

wget --quiet --user-agent="${UA}" -O ${TMP_DIR}/aarch64_Image ${BASE}/kernel_aarch64/latest/raw/Image
wget --quiet --user-agent="${UA}" -O ${TMP_DIR}/aarch64_16k_Image ${BASE}/kernel_aarch64_16k/latest/raw/Image

head -c 64 ${TMP_DIR}/aarch64_Image > ${DATA_DIR}/aarch64_4k_header.bin
head -c 64 ${TMP_DIR}/aarch64_16k_Image > ${DATA_DIR}/aarch64_16k_header.bin
dd if=/dev/zero of=${DATA_DIR}/aarch64_invalid_header.bin bs=64 count=1 status=none
