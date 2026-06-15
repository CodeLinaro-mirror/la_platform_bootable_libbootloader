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
"""QEMU trace generation test script"""

import logging
import os
from pathlib import Path
import sys
import time
from qemu_test_utils import default_logging, wait_for_log_pattern

GBL_TRACE_MAGIC = 0x0641DAC6BD9D2EA3


def main():
  default_logging()
  logging.info("Starting trace generation test...")

  console_log_path = os.environ.get("GBL_CONSOLE_LOG")
  assert console_log_path, "GBL_CONSOLE_LOG not set"

  # Wait for trace.bin to be written via semihosting, and read/validate it.
  trace = Path("trace.bin")
  # Caller of the test script (qemu_launcher.py) enforces timeout.
  while True:
    try:
      assert trace.read_bytes()[:8] == GBL_TRACE_MAGIC.to_bytes(8, "little")
      break
    except Exception as e:
      logging.info(f"Failed checking for trace.bin {e}. Retrying...")
      pass
    time.sleep(0.5)

  # Wait for the console log to confirm clean exit
  wait_for_log_pattern(
      console_log_path,
      [r"^\[\d+\.\d+\] Exiting QEMU test via semihosting\.$"],
  )

  logging.info("Trace test completed successfully.")


if __name__ == "__main__":
  main()
