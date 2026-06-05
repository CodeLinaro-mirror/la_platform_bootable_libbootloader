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

  parser.add_argument("efi", help="Path to the GBL launcher EFI application")
  parser.add_argument("gbl", help="Path to the GBL binary")
  parser.add_argument("--bios", help="Path to the BIOS (UEFI firmware)")
  parser.add_argument("--qemu", help="Path to the QEMU binary")
  parser.add_argument(
      "--timeout", type=int, help="timeout in seconds", default=10
  )
  parser.add_argument("--log_output", help="Output path for serial log")
  parser.add_argument(
      "--disk",
      action="append",
      help="Path to a disk image to attach as virtio-blk",
  )
  parser.add_argument(
      "--vhost_device_vsock", help="Path to the vhost device vsock binary"
  )
  parser.add_argument(
      "--test_script", help="Path to a user-provided Python script to execute"
  )

  return parser.parse_args()


def launch_qemu(args):
  qemu = os.path.abspath(args.qemu)
  bios = os.path.abspath(args.bios)
  with tempfile.TemporaryDirectory() as test_dir:
    env = os.environ.copy()
    # Flushes any log immediately.
    env["PYTHONUNBUFFERED"] = "1"
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

    gbl_path = os.path.abspath(args.gbl)
    os.symlink(gbl_path, test_dir / "gbl.bin")

    # Starts vhost device vsock bridge first. Otherwise QEMU will fail to start.
    socket_path = test_dir / "vsock-guest.sock"
    uds_path = test_dir / "vsock-host.sock"

    # Shares the fastboot vsock socket path and log path to test script.
    env["FASTBOOT_OVER_VSOCK_UDS_PATH"] = str(uds_path)
    env["GBL_CONSOLE_LOG"] = str(test_dir / "console.log")
    script_log_path = test_dir / "test_script.log"

    if args.vhost_device_vsock:
      vhost_proc = subprocess.Popen(
          [
              os.path.abspath(args.vhost_device_vsock),
              "--vm",
              f"guest-cid=3,socket={socket_path},uds-path={uds_path}",
          ],
          stderr=subprocess.STDOUT,
          cwd=test_dir,
          env=env,
      )

    try:
      cmd_args = [qemu, "-nographic", "-cpu", "max"]
      cmd_args += [
          "-m",
          "256M",  # 256mb is minimum requirement by edk2
          "-object",
          "memory-backend-memfd,id=mem,size=256M,share=on",
      ]
      # Skips the 5 seconds delay spent waiting for user input in the boot menu
      cmd_args += ["-boot", "menu=on,splash-time=0"]
      # EDK2 firmware
      cmd_args += ["-bios", bios]
      # ESP partition
      cmd_args += ["-drive", "format=raw,file=fat:rw:esp"]
      # Add extra disks
      disks = args.disk or []
      for i, disk in enumerate(disks):
        # GBL needs read/write access to the disk image.
        # Bazel output artifacts are read-only, so create a copy of the disk
        # image.
        disk_path = test_dir / f"disk_{i}.img"
        shutil.copyfile(disk, disk_path)
        os.chmod(disk_path, 0o644)
        drive_id = f"hd{i}"
        cmd_args += [
            "-drive",
            f"file={disk_path},format=raw,if=none,id={drive_id}",
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
      # userspace vsock interface
      if args.vhost_device_vsock:
        cmd_args += [
            "-chardev",
            f"socket,id=char0,reconnect=0,path={socket_path}",
        ]
        cmd_args += ["-device", "vhost-user-vsock-pci,chardev=char0"]

      # Generate FDT
      subprocess.run(
          cmd_args + ["-machine", "virt,dumpdtb=fdt.dtb,memory-backend=mem"],
          check=True,
          stderr=subprocess.STDOUT,
          cwd=test_dir,
          env=env,
      )

      # Launch QEMU
      failed = False
      qemu_proc = subprocess.Popen(
          cmd_args + ["-machine", "virt,memory-backend=mem"],
          stderr=subprocess.STDOUT,
          cwd=test_dir,
          env=env,
      )

      # Run test script if provided
      #
      # Notes: The launching and management of qemu can also be driven by the
      # test script. This may allow the test script to be written like unittest.
      # For example, a test scripts may contain several python unittest and each
      # test launches its own instance of qemu.
      if args.test_script:
        with open(script_log_path, "w") as script_log:
          subprocess.run(
              [sys.executable, os.path.abspath(args.test_script)],
              timeout=args.timeout,
              check=True,
              stdout=script_log,
              stderr=subprocess.STDOUT,
              cwd=test_dir,
              env=env,
          )

      # Wait for QEMU to exit
      qemu_proc.wait(timeout=args.timeout)
      if qemu_proc.returncode != 0:
        raise subprocess.CalledProcessError(
            qemu_proc.returncode, qemu_proc.args
        )
    except Exception as e:
      failed = True
      print(f"QEMU error: {e}")
      raise
    finally:
      qemu_proc.terminate()
      qemu_proc.wait()
      if args.vhost_device_vsock:
        vhost_proc.terminate()
        vhost_proc.wait()
      if args.log_output:
        with open(args.log_output, "w") as outfile:
          outfile.write("=== Device Console Log ===\n")
          outfile.write((test_dir / "console.log").read_text())
          if script_log_path.exists():
            outfile.write("\n=== Host Test Script Log ===\n")
            outfile.write(script_log_path.read_text())
        if failed:
          print(f"\nQEMU Test Failed! Output log:\n")
          log_text = pathlib.Path(args.log_output).read_text()
          # Strip ANSI escape codes (like clear screen)
          clean_text = re.sub(r"\x1b\[[0-9;]*[mGHKJ]", "", log_text)
          print(clean_text)


if __name__ == "__main__":
  args = parse_args()
  launch_qemu(args)
  sys.exit(0)
