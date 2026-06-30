#!/usr/bin/env python3
# Copyright (C) 2026 The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#       http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""Integration test for GBL debugging using LLDB."""

import logging
import os
import pathlib
import sys
import time
import lldb
import qemu_test_utils


def sync_events(debugger):
  """Consume all pending events to update the process state internally.

  This is necessary when calling lldb python APIs in async mode, where there
  isn't a background loop running to consume events.
  """
  listener = debugger.GetListener()
  event = lldb.SBEvent()
  while listener.GetNextEvent(event):
    state = lldb.SBProcess.GetStateFromEvent(event)
    if state != lldb.eStateInvalid:
      logging.info(f"[Event] Process state changed to: {state}")


def wait_for_stop_state(debugger, process):
  """Wait for the process to transition to the Stopped state."""
  logging.info("Waiting for process to be in Stopped state")
  while process.GetState() != lldb.eStateStopped:
    sync_events(debugger)
    time.sleep(0.05)


if __name__ == "__main__":
  qemu_test_utils.default_logging()

  # Initialize LLDB
  debugger = lldb.SBDebugger.Create()
  debugger.SetAsync(False)

  # Configure target architecture.
  error = lldb.SBError()
  target = debugger.CreateTarget(
      "gbl.bin", "aarch64-pc-windows-msvc", None, False, error
  )
  assert target.IsValid(), f"Failed to create target: {error.GetCString()}"

  # Look for the GDB unix domain socket channel created by the qemu launcher.
  gdb_socket = os.environ.get("GBL_GDB_SOCKET")
  assert gdb_socket is not None, "GBL_GDB_SOCKET env variable is not set"
  socket_url = f"unix-connect://{os.path.abspath(gdb_socket)}"
  logging.info(f"Connecting to {socket_url} using process connect...")
  return_obj = lldb.SBCommandReturnObject()
  debugger.GetCommandInterpreter().HandleCommand(
      f"process connect --plugin gdb-remote {socket_url}", return_obj
  )
  assert return_obj.Succeeded(), f"connect failed: {return_obj.GetError()}"

  logging.info("Waiting for debugging client to be ready...")
  process = target.GetProcess()
  assert process.IsValid(), "Failed to get valid process"
  wait_for_stop_state(debugger, process)

  logging.info("Continuing QEMU to load GBL...")
  # Enter async mode so that we can run qemu while checking logs.
  debugger.SetAsync(True)
  cont_err = process.Continue()
  assert cont_err.Success(), f"Failed to continue: {cont_err.GetCString()}"

  # Poll the console log for the loaded base address printed by gbl_launcher.
  console_log_path = os.environ.get("GBL_CONSOLE_LOG")
  assert console_log_path is not None, "GBL_CONSOLE_LOG is not set"
  logging.info(f"Polling console {console_log_path} for GBL load address...")
  matches = qemu_test_utils.wait_for_log_pattern(
      console_log_path,
      [r"GBL loaded at (0x[0-9a-fA-F]+):"],
  )
  base_addr = int(matches[0][0].group(1), 16)
  logging.info(f"Detected GBL loaded at base address: {base_addr:#x}")

  # Sync events to update LLDB's internal process state to running
  sync_events(debugger)

  # Stop QEMU execution to set up breakpoint
  logging.info("Stopping QEMU to configure debugging symbols...")
  stop_err = process.Stop()
  assert stop_err.Success(), f"Failed to stop: {stop_err.GetCString()}"
  wait_for_stop_state(debugger, process)

  # Figure out the base address specified in PE/COFF header to compute
  # relatively offset w.r.t actual load address.
  module = target.GetModuleAtIndex(0)
  assert module.IsValid(), "Failed to get valid module"
  sec = module.GetSectionAtIndex(0)
  assert sec.IsValid(), "Failed to get valid section"
  default_base = sec.GetFileAddress() - sec.GetFileOffset()
  logging.info(f"Detected default image base: {default_base:#x}")
  offset = (base_addr - default_base) % (1 << 64)

  # Loads the binary and symbol according to the relative load offset.
  logging.info(f"Loading image at offset: {offset:#x}")
  debugger.HandleCommand(f"image load -f gbl.bin -s {offset:#x}")
  debugger.HandleCommand("target symbols add gbl.pdb")

  # Set breakpoint at function "efi_main"
  bp_efi_main = target.BreakpointCreateByName("efi_main")
  assert (
      bp_efi_main.IsValid() and bp_efi_main.GetNumLocations() > 0
  ), "Failed to set breakpoint at efi_main"
  logging.info(
      f"Breakpoint set at efi_main with {bp_efi_main.GetNumLocations()}"
      " locations."
  )

  # Write the marker file to release gbl_launcher from its wait loop
  logging.info("Creating gdb.ready marker to release gbl_launcher...")
  pathlib.Path("gdb.ready").touch()

  # Continue execution. It should run and hit the efi_main breakpoint directly.
  debugger.SetAsync(False)
  logging.info("Continuing execution to efi_main breakpoint (blocking)...")
  cont_err = process.Continue()
  assert cont_err.Success(), f"Failed to continue: {cont_err.GetCString()}"

  # Verify the efi_main breakpoint was hit
  state = process.GetState()
  assert state == lldb.eStateStopped, "Process is not stopped"
  thread = process.GetSelectedThread()
  assert thread.IsValid(), "Invalid selected thread"
  stop_reason = thread.GetStopReason() if thread.IsValid() else None
  assert (
      stop_reason == lldb.eStopReasonBreakpoint
  ), f"Unexpected stop reason: {stop_reason}"
  frame = thread.GetSelectedFrame()
  pc_val = frame.GetPC() if frame.IsValid() else None
  logging.info(
      f"Stopped. State: {state}, Stop Reason: {stop_reason}, PC:"
      f" {hex(pc_val) if pc_val is not None else None}, Function:"
      f" {frame.GetFunctionName()}"
  )

  # Verify the function object containing the PC is efi_main
  func = frame.GetFunction()
  assert func.IsValid(), "Invalid concrete function in frame"
  logging.info(f"Breakpoint hit at frame function: {func.GetName()}")
  assert (
      "efi_main" in func.GetName()
  ), f"Expected to stop at efi_main, got {func.GetName()}"

  logging.info("Continuing execution and waiting for target to exit...")
  cont_err = process.Continue()
  assert cont_err.Success(), f"Failed to continue: {cont_err.GetCString()}"

  # Verify the target is indeed exited or detached
  state = process.GetState()
  logging.info(f"Target process state after run: {state}")
  assert state in [
      lldb.eStateExited,
      lldb.eStateDetached,
  ], f"Unexpected final target state: {state}"

  # Verify kernel exit log in the console log
  qemu_test_utils.wait_for_kernel_exit(console_log_path)
