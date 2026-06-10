#!/bin/bash
#
# Copyright (C) 2024 The Android Open Source Project
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
readonly DATA_DIR="${SCRIPT_DIR}/data/"
readonly TMP_DIR=`mktemp -d`

dtc -I dts -O dtb -o ${TMP_DIR}/a.dtb ${SCRIPT_DIR}/a.dts
dtc -I dts -O dtb -o ${TMP_DIR}/b.dtb ${SCRIPT_DIR}/b.dts
dtc -I dts -O dtb -o ${TMP_DIR}/c.dtb ${SCRIPT_DIR}/c.dts
dtc -I dts -O dtb -o ${TMP_DIR}/d.dtb ${SCRIPT_DIR}/d.dts

echo "corrupted dttable" > ${DATA_DIR}/corrupted_dttable.img

# Use the python script in external/libufdt
readonly MKDTBOIMG="python3 ${SCRIPT_DIR}/../../../../../external/libufdt/utils/src/mkdtboimg.py"

$MKDTBOIMG create ${DATA_DIR}/dttable_v0.img --version=0 \
  ${TMP_DIR}/a.dtb --id=0x0 --rev=0x0 --custom0=0x0 --custom1=0x1 --custom2=0x2 --custom3=0x3 \
  ${TMP_DIR}/b.dtb --id=0x1 --rev=0x0 --custom0=0x0 --custom1=0x1 --custom2=0x2 --custom3=0x3 \
  ${TMP_DIR}/c.dtb --id=0x2 --rev=0x0 --custom0=0x0 --custom1=0x1 --custom2=0x2 --custom3=0x3 \
  ${TMP_DIR}/d.dtb --id=0x3 --rev=0x0 --custom0=0x0 --custom1=0x1 --custom2=0x2 --custom3=0x3

$MKDTBOIMG create ${DATA_DIR}/dttable_v1.img --version=1 \
  ${TMP_DIR}/a.dtb --id=0x0 --rev=0x0 --flags=0x10 --custom0=0x0 --custom1=0x1 --custom2=0x2 \
  ${TMP_DIR}/b.dtb --id=0x1 --rev=0x0 --flags=0x10 --custom0=0x0 --custom1=0x1 --custom2=0x2 \
  ${TMP_DIR}/c.dtb --id=0x2 --rev=0x0 --flags=0x10 --custom0=0x0 --custom1=0x1 --custom2=0x2 \
  ${TMP_DIR}/d.dtb --id=0x3 --rev=0x0 --flags=0x10 --custom0=0x0 --custom1=0x1 --custom2=0x2

$MKDTBOIMG create ${DATA_DIR}/dttable_v2.img --version=2 \
  ${TMP_DIR}/a.dtb --id=0x0 --rev=0x0 --flags=0x20 --custom0=0x0 --custom1=0x1 --custom2=0x2 \
  --custom3=0x3 --custom4=0x4 --custom5=0x5 --custom6=0x6 --custom7=0x7 --custom8=0x8 --custom9=0x9 \
  --custom10=0xa \
  ${TMP_DIR}/b.dtb --id=0x1 --rev=0x0 --flags=0x20 --custom0=0x0 --custom1=0x1 --custom2=0x2 \
  --custom3=0x3 --custom4=0x4 --custom5=0x5 --custom6=0x6 --custom7=0x7 --custom8=0x8 --custom9=0x9 \
  --custom10=0xa \
  ${TMP_DIR}/c.dtb --id=0x2 --rev=0x0 --flags=0x20 --custom0=0x0 --custom1=0x1 --custom2=0x2 \
  --custom3=0x3 --custom4=0x4 --custom5=0x5 --custom6=0x6 --custom7=0x7 --custom8=0x8 --custom9=0x9 \
  --custom10=0xa \
  ${TMP_DIR}/d.dtb --id=0x3 --rev=0x0 --flags=0x20 --custom0=0x0 --custom1=0x1 --custom2=0x2 \
  --custom3=0x3 --custom4=0x4 --custom5=0x5 --custom6=0x6 --custom7=0x7 --custom8=0x8 --custom9=0x9 \
  --custom10=0xa

echo "Dumping v0:"
$MKDTBOIMG dump ${DATA_DIR}/dttable_v0.img
echo "Dumping v1:"
$MKDTBOIMG dump ${DATA_DIR}/dttable_v1.img
echo "Dumping v2:"
$MKDTBOIMG dump ${DATA_DIR}/dttable_v2.img

