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
"""QEMU trace analysis test script

The script tests:

1. GBL trace collection and conversion to trace event format used by perfetto.
2. Enforce maximum stack usage in currently known critical path "fastboot boot".
"""

import json
import logging
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
from python.runfiles import runfiles
from qemu_test_utils import (
    VsockFastbootClient,
    default_logging,
    wait_for_fastboot_ready,
    wait_for_log_pattern,
)

GBL_TRACE_MAGIC = 0x0641DAC6BD9D2EA3
# At the time this script is written, "fastboot boot" is the critical path that
# causes the highest stack use of 86176 bytes. Set to 128K bytes for now.
MAX_STACK_ALLOWED = 128 * 1024  # 128k bytes


def main():
  default_logging()
  logging.info("Starting trace generation test...")

  console_log_path = os.environ.get("GBL_CONSOLE_LOG")
  assert console_log_path, "GBL_CONSOLE_LOG not set"

  # Wait for device to be ready before connecting to vsock. Otherwise
  # vhost-device-vsock may become out of sync if VsockFastbootClient
  # timeout first.
  wait_for_fastboot_ready(console_log_path)

  client = VsockFastbootClient(port=1)
  client.download("boot_a.img")
  client.run_command(b"boot", assert_ok=True)
  client.close()

  # Wait for the console log to confirm clean exit
  wait_for_log_pattern(
      console_log_path,
      [r"^\[\d+\.\d+\] Exiting QEMU test via semihosting\.$"],
  )

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

  outputs_dir = os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR")
  if outputs_dir:
    outputs_path = Path(outputs_dir)
    shutil.copy(trace, outputs_path / "trace.bin")
    logging.info(f"Copied trace.bin to artifact directory: {outputs_dir}")

    # Convert trace.bin to perfetto trace format.
    r = runfiles.Create()
    script_path = r.Rlocation("gbl/tools/gbl-trace-to-perfetto.py")
    symbolizer = r.Rlocation("llvm_linux_x86_64_prebuilts/bin/llvm-symbolizer")
    assert script_path, "Could not find gbl-trace-to-perfetto.py in runfiles"
    assert symbolizer, "Could not find llvm-symbolizer in runfiles"
    perfetto_out = outputs_path / "trace.perfetto"

    logging.info("Running gbl-trace-to-perfetto.py...")
    start_time = time.perf_counter()
    subprocess.run(
        [
            sys.executable,
            str(script_path),
            "trace.bin",
            "gbl.bin",
            str(perfetto_out),
            "--llvm-symbolizer",
            str(symbolizer),
        ],
        check=True,
    )

    duration = time.perf_counter() - start_time
    logging.info(
        f"Generated perfetto trace at: {perfetto_out} (took {duration:.2f}s)"
    )

    # Check stack usage. If not meet the limit, print error message and exit.
    perfetto_data = json.loads(perfetto_out.read_text())
    max_stack = 0
    for event in perfetto_data:
      if "args" in event and "stack usage" in event["args"]:
        max_stack = max(max_stack, event["args"]["stack usage"])
    logging.info(f"Maximum stack usage found in trace: {max_stack} bytes")
    artifacts_out = os.environ.get("TEST_ARTIFACTS_OUT")
    if max_stack > MAX_STACK_ALLOWED:
      logging.error(
          f"GBL stack usage limit exceeded: {max_stack} bytes used, "
          f"limit is {MAX_STACK_ALLOWED} bytes. To inspect the detailed"
          " callgraph, "
          'open https://ui.perfetto.dev/ and load the "trace.perfetto" file '
          f'extracted from "{artifacts_out}".'
      )
      sys.exit(1)

  logging.info("Trace test completed successfully.")


if __name__ == "__main__":
  main()
