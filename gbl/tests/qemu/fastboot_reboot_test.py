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
"""QEMU fastboot reboot test script"""

import os
import sys
from qemu_test_utils import VsockFastbootClient, wait_for_log_pattern


def main():
  print("Starting fastboot reboot test...")
  uds_path = os.environ.get("FASTBOOT_OVER_VSOCK_UDS_PATH")
  if not uds_path:
    print("Error: VSOCK UDS path not specified in environment variable.")
    sys.exit(1)

  port = 1
  client = VsockFastbootClient(port=port)
  client.run_command(b"getvar:all", assert_ok=True)
  client.run_command(b"reboot-bootloader", assert_ok=True)

  client = VsockFastbootClient(port=port)
  client.run_command(b"reboot-recovery", assert_ok=True)

  # Poll the console log file for success pattern
  console_log_path = os.environ.get("GBL_CONSOLE_LOG")
  assert console_log_path, "GBL_CONSOLE_LOG not set"
  wait_for_log_pattern(
      console_log_path,
      [
          r"^\[\d+\.\d+\] Normal Mode: false$",
          r"^\[\d+\.\d+\] Exiting QEMU test via semihosting\.$",
      ],
  )


if __name__ == "__main__":
  main()
