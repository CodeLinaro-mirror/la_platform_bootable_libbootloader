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
import os
import time
import gdb

# Must be kept in sync with `efi/app/main.rs`
MAGIC = 0x166EB3328561CFE7


# Returns the selected inferior's architecture
def arch():
  return gdb.selected_inferior().architecture().name()


# key: <arch name>, val: (magic register, base address register)
ARCH_HANDSHAKE_REGISTERS = {
    "i386:x86-64": ("rax", "rcx"),
    "aarch64": ("x0", "x2"),
}


def arch_handshake_registers():
  return ARCH_HANDSHAKE_REGISTERS[arch()]


# Define a new GDB command "load_gbl_debug_bin"
#
# The command waits until GBL sets $rax=MAGIC and $rcx=<load address>, then
# loads the symbol and breaks the program for users to start debugging.
class LoadGblDebugBinCommand(gdb.Command):
  """A simple command to greet the user from GDB."""

  def __init__(self):
    # Registers a new command "load_gbl_debug_bin"
    super(LoadGblDebugBinCommand, self).__init__(
        "load_gbl_debug_bin", gdb.COMMAND_USER
    )

  def scan_all_threads(self):
    # In case qemu is configured with multiple CPUs, we need to scan all of
    # them for $rax==`MAGIC` to determine which one GBL is running on.
    for inferior in gdb.inferiors():
      (magic_reg, _) = arch_handshake_registers()
      for thread in inferior.threads():
        thread.switch()
        if int(gdb.parse_and_eval(f"${magic_reg}")) == MAGIC:
          print(f"Target Thread ID: {thread.global_num}")
          return True
    return False

  def invoke(self, arg, from_tty):
    # Prevents UI interaction due to pagination.
    gdb.execute("set pagination off")
    while not self.scan_all_threads():
      run_wait_secs = 3
      print(f"Run {run_wait_secs} seconds for GBL to be debug ready.")

      def delayed_interrupt():
        time.sleep(run_wait_secs)
        gdb.execute("interrupt")

      gdb.post_event(delayed_interrupt)
      gdb.execute("continue")

    (magic_reg, base_reg) = arch_handshake_registers()
    base = int(gdb.parse_and_eval(f"${base_reg}"))
    print(f"GBL connected. Take image base address from {base_reg}: {base:#x}")

    # Loads the symbol according to actual GBL load address.
    #
    # Currently we assume that the image base address in the PE/COFF header is
    # always set to 0x140000000 by the compiler. So far this has been the case.
    # If this no longer holds, we need to parse the PE image to get it.
    offset = (base - 0x0000000140000000) % (1 << 64)
    gdb.execute(f"add-symbol-file {arg} -o {offset:#x}")
    gdb.execute(f"set ${magic_reg} = 0")  # Exits the wait loop in `wait_gdb()`
    gdb.execute("frame")  # Shows current exeuctaion code location
    print("You can now run `next`, `step`, 'hb' etc to start debugging.")
    gdb.execute("set pagination on")


LoadGblDebugBinCommand()
