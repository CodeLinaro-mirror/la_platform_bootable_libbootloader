#!/usr/bin/env python3
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
"""QEMU DICE boot test script"""

import logging
import os
from qemu_test_utils import (
    default_logging,
    wait_for_kernel_exit,
    wait_for_log_pattern,
)


def main():
  default_logging()
  logging.info("Starting DICE boot test...")

  console_log_path = os.environ.get("GBL_CONSOLE_LOG")
  assert console_log_path, "GBL_CONSOLE_LOG not set"

  expected_alg = os.environ.get("GBL_DICE_ALG")
  assert expected_alg, "GBL_DICE_ALG not set"
  logging.info(f"Expected DICE algorithm: {expected_alg}")

  # Verify that GBL derives the DICE handover and that the kernel extracts the
  # expected subject key algorithm from it.
  wait_for_log_pattern(
      console_log_path,
      [
          rf"^\[\d+\.\d+\] \[DICE-TEST\] Extracted algorithm: {expected_alg}$",
      ],
  )

  wait_for_kernel_exit(console_log_path)
  logging.info("QEMU DICE boot test completed successfully.")


if __name__ == "__main__":
  main()
