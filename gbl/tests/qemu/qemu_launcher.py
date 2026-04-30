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
"""QEMU Test Launcher"""

import argparse
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile


def parse_args() -> argparse.Namespace:
  parser = argparse.ArgumentParser(
      description=__doc__,
      formatter_class=argparse.RawDescriptionHelpFormatter,
  )

  parser.add_argument("efi", help="Path to the EFI application")
  parser.add_argument("--bios", help="Path to the BIOS (UEFI firmware)")
  parser.add_argument("--qemu", help="Path to the QEMU binary")
  parser.add_argument(
      "--timeout", type=int, help="timeout in seconds", default=10
  )
  parser.add_argument("--log_output", help="Output path for serial log")
  parser.add_argument(
      "--disk", action="append", help="Path to a disk image to attach as virtio-blk"
  )

  return parser.parse_args()


def launch_qemu(args):
  qemu = os.path.abspath(args.qemu)
  bios = os.path.abspath(args.bios)
  with tempfile.TemporaryDirectory() as test_dir:
    env = os.environ.copy()
    # The script will be run in a sandbox, so we need to set the temp dir.
    env["TMPDIR"] = test_dir
    env["TEMP"] = test_dir
    env["TMP"] = test_dir
    test_dir = pathlib.Path(test_dir)
    # Create a FAT filesystem image for the EFI System Partition (ESP)
    esp_part_dir = test_dir / "esp" / "EFI" / "BOOT"
    esp_part_dir.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(args.efi, esp_part_dir / "bootaa64.efi")
    # Make sure a log file always eixsts
    (test_dir / "console.log").write_text("")
    failed = False
    try:
      cmd_args = [qemu, "-nographic", "-machine", "virt", "-cpu", "max"]
      cmd_args += ["-m", "256M"]  # 256mb is minimum requirement by edk2
      # Skips the 5 seconds delay spent waiting for user input in the boot menu
      cmd_args += ["-boot", "menu=on,splash-time=0"]
      # EDK2 firmware
      cmd_args += ["-bios", bios]
      # ESP partition
      cmd_args += ["-drive", "format=raw,file=fat:rw:esp"]
      # Add extra disks
      disks = args.disk or []
      for i, disk in enumerate(disks):
        disk_path = os.path.abspath(disk)
        drive_id = f"hd{i}"
        cmd_args += [
            "-drive",
            f"file={disk_path},format=raw,if=none,id={drive_id},readonly=on",
        ]
        cmd_args += ["-device", f"virtio-blk-device,drive={drive_id}"]
      # Re-direct all sources of serial log to a log file
      cmd_args += ["-serial", "chardev:console"]
      cmd_args += ["-monitor", "chardev:console"]
      cmd_args += ["-semihosting"]
      cmd_args += ["-semihosting-config", "chardev=console"]
      cmd_args += [
          "-chardev",
          "socket,id=console,path=con_in.sock,server=on,wait=off,mux=on,logfile=console.log",
      ]
      subprocess.run(
          cmd_args,
          timeout=args.timeout,
          check=True,
          stderr=subprocess.STDOUT,
          cwd=test_dir,
          env=env,
      )
    except Exception as e:
      failed = True
      print(f"QEMU error: {e}")
      raise
    finally:
      if args.log_output:
        shutil.copyfile(test_dir / "console.log", args.log_output)
        if failed:
          print(f"\nQEMU Test Failed! Console log:\n")
          log_text = (test_dir / "console.log").read_text()
          # Strip ANSI escape codes (like clear screen)
          clean_text = re.sub(r"\x1b\[[0-9;]*[mGHKJ]", "", log_text)
          print(clean_text)


if __name__ == "__main__":
  args = parse_args()
  launch_qemu(args)
  sys.exit(0)
