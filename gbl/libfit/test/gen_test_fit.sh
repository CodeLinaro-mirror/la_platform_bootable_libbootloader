#!/bin/bash
#
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

set -e


readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
readonly DATA_DIR="${SCRIPT_DIR}/data/"

dtc -I dts -O dtb -o ${DATA_DIR}/metadata.dtb -a 8 ${SCRIPT_DIR}/metadata.dts
dtc -@ -I dts -O dtb -o ${DATA_DIR}/platform-1.dtb -a 8 ${SCRIPT_DIR}/platform-1.dts
dtc -@ -I dts -O dtb -o ${DATA_DIR}/platform-2.dtb -a 8 ${SCRIPT_DIR}/platform-2.dts
dtc -@ -I dts -O dtb -o ${DATA_DIR}/overlay-1.dtb -a 8 ${SCRIPT_DIR}/overlay-1.dts
dtc -@ -I dts -O dtb -o ${DATA_DIR}/overlay-2.dtb -a 8 ${SCRIPT_DIR}/overlay-2.dts

#Creation of fit image in .img format
mkimage -f ${SCRIPT_DIR}/fitimage.its ${DATA_DIR}/fit.img -E -B 8
mkimage -f ${SCRIPT_DIR}/fitimage_with_default_option.its ${DATA_DIR}/fit_with_default_option.img -E -B 8
mkimage -f ${SCRIPT_DIR}/fitimage_with_no_metadata.its ${DATA_DIR}/fit_with_no_metadata.img -E -B 8
mkimage -f ${SCRIPT_DIR}/fitimage_with_invalid_metadata_type.its ${DATA_DIR}/fit_with_invalid_metadata_type.img -E -B 8
mkimage -f ${SCRIPT_DIR}/fitimage_with_invalid_metadata_position.its ${DATA_DIR}/fit_with_invalid_metadata_position.img -E -B 8

#Creation of zeros.img
dd if=/dev/zero of=${DATA_DIR}/zeros.img count=10 bs=1M
