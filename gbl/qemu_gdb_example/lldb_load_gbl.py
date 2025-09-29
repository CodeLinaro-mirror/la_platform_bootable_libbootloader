#!/usr/bin/env python3
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
"""Custom GDB command for loading GBL debug symbols"""

import argparse
import pathlib
import re
import time
import lldb

SCRIPT_DIR = pathlib.Path(__file__).resolve().parent
AOSP_ROOT = SCRIPT_DIR.parents[3]
GBL_ROOT = SCRIPT_DIR.parents[1]

# Must be kept in sync with `efi/app/main.rs`
MAGIC = 0x166EB3328561CFE7

# key: <arch name>, val: (magic register, base address register)
ARCH_HANDSHAKE_REGISTERS = {
    "x86_64-pc-windows-msvc": ("rax", "rcx"),
    "aarch64-pc-windows-msvc": ("x0", "x2"),
}


def arch_handshake_registers():
  return ARCH_HANDSHAKE_REGISTERS[lldb.target.triple]


# A heuristic for finding the rust toolchain version.
def gbl_rust_ver():
  text = (GBL_ROOT / "gbl" / "toolchain" / "gbl_workspace_util.bzl").read_text()
  for l in text.splitlines():
    res = re.match('GBL_RUST_VERSION = "(.*)"', l)
    if not res:
      continue
    return res.group(1)
  return None


def scan_all_threads():
  # In case qemu is configured with multiple CPUs, we need to scan all of
  # them for `MAGIC` value to determine which one GBL is running on.
  (reg, _) = arch_handshake_registers()
  for t in range(0, lldb.process.GetNumThreads()):
    thrd = lldb.process.GetThreadAtIndex(t)
    val = thrd.GetSelectedFrame().FindRegister(reg).GetValue()
    if int(val, 16) == MAGIC:
      print(f"Selected thread {t}")
      lldb.process.SetSelectedThreadByIndexID(t)
      return True
  return False


def start_gbl():
  efi = str(lldb.target.GetExecutable())
  pdb = pathlib.Path(efi).with_suffix(".pdb")
  lldb.debugger.SetAsync(True)
  while not scan_all_threads():
    run_wait_secs = 3
    print(f"Run {run_wait_secs} seconds for GBL to be debug ready.")
    lldb.process.Continue()
    time.sleep(run_wait_secs)
    lldb.process.Stop()

  (magic_reg, base_reg) = arch_handshake_registers()
  base = int(
      lldb.process.GetSelectedThread()
      .GetSelectedFrame()
      .FindRegister(base_reg)
      .GetValue(),
      16,
  )
  print("GBL is debug ready")
  print(f"Image base address: {base:#x}")

  # Loads the symbol according to actual GBL load address.
  #
  # Currently we assume that the image base address in the PE/COFF header is
  # always set to 0x140000000 by the compiler. So far this has been the case.
  # If this no longer holds, we need to parse the PE image to get it.
  offset = (base - 0x0000000140000000) % (1 << 64)
  lldb.debugger.HandleCommand(f"image load -f {efi} -s {offset:#x}")
  lldb.debugger.HandleCommand(f"target symbols add {pdb}")
  # Sets source path
  source_map = ""
  rust_ver = gbl_rust_ver()
  if rust_ver:
    source_map += (
        " /external/rust_prebuilts/"
        f" {AOSP_ROOT}/prebuilts/rust/linux-x86/{rust_ver}"
    )
  source_map += f" /external/ {GBL_ROOT}"
  lldb.debugger.HandleCommand(f"setting set target.source-map {source_map}")
  lldb.process.GetSelectedThread().GetSelectedFrame().FindRegister(
      magic_reg
  ).SetValueFromCString("0x0")
