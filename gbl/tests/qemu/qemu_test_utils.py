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
"""Contains utils for writing qemu test scripts"""

import os
import re
import socket
import sys
import time


def recv_exact(sock: socket.socket, num_bytes: int) -> bytes:
  buf = bytearray(num_bytes)
  view = memoryview(buf)
  # MSG_WAITALL tells the OS to wait until the buffer is completely full
  sock.recv_into(view, num_bytes, socket.MSG_WAITALL)
  return bytes(buf)


class VsockFastbootClient:
  """Fastboot client communicating over vhost-device-vsock Unix Domain Sockets."""

  def __init__(
      self, port: int, uds_path: str = None, timeout_secs: float = 15.0
  ):
    self.uds_path = uds_path or os.environ.get("FASTBOOT_OVER_VSOCK_UDS_PATH")
    self.port = port
    self.timeout = timeout_secs
    self.sock = self._connect_vsock()
    self._handshake_fb01()

  def _connect_vsock(self) -> socket.socket:
    end_time = time.time() + self.timeout
    while time.time() < end_time:
      try:
        print(f"Connecting to {self.uds_path}, destination port {self.port}...")
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(2)
        s.connect(self.uds_path)
        s.sendall(f"CONNECT {self.port}\n".encode("utf-8"))
        header = s.recv(1024)
        print("Received header", header)
        if header.startswith(b"OK"):
          return s
        s.close()
      except Exception as e:
        print(f"Connection attempt exception: {type(e).__name__}: {e}")
      time.sleep(0.5)
    raise ConnectionError(
        f"Failed to establish VSock connection to port {self.port}"
    )

  def _handshake_fb01(self):
    print("VSock connection established. Initiating FB01 handshake...")
    self.sock.sendall(b"FB01")
    resp = recv_exact(self.sock, 4)
    if resp != b"FB01":
      raise ConnectionError(f"Handshake error, expected FB01, got {resp}")
    print("FB01 handshake successful.")

  def send(self, cmd: bytes):
    """Sends length-prefixed packet."""
    length_prefix = len(cmd).to_bytes(8, byteorder="big")
    self.sock.sendall(length_prefix + cmd)

  def recv(self) -> bytes:
    """Receives length-prefixed packet."""
    length_prefix = recv_exact(self.sock, 8)
    length = int.from_bytes(length_prefix, byteorder="big")
    return recv_exact(self.sock, length)

  def run_command(
      self, cmd: bytes, assert_ok: bool = False
  ) -> tuple[bytes, list[bytes]]:
    """Sends a fastboot command and reads the response."""
    print(f"Sending command: {cmd}")
    self.send(cmd)
    info_messages = []
    while True:
      reply = self.recv()
      print(f"Received: {reply}")
      if reply.startswith(b"INFO"):
        info_messages.append(reply)
        continue
      if assert_ok:
        assert reply.startswith(b"OKAY"), f"{cmd} failed: {reply}"
      return reply, info_messages

  def close(self):
    self.sock.close()


def wait_for_log_pattern(
    log_path: str, patterns: list[str], timeout_secs: float = 15.0
):
  """Wait for a list of regex patterns to appear in consecutive lines in the log file"""
  print(f"Waiting for consecutive patterns {patterns} in console log")
  regexes = [re.compile(p) for p in patterns]
  end_time = time.time() + timeout_secs
  while time.time() < end_time:
    if os.path.exists(log_path):
      try:
        with open(log_path, "r", errors="ignore") as f:
          lines = f.readlines()
          num_lines = len(lines)
          num_patterns = len(regexes)

          for i in range(num_lines - num_patterns + 1):
            match = True
            for j in range(num_patterns):
              if not regexes[j].search(lines[i + j]):
                match = False
                break
            if match:
              print("All patterns matched consecutively in console log!")
              return
      except Exception as e:
        print(f"Warning: Failed to read console log file: {e}")
    time.sleep(0.2)

  raise TimeoutError(
      f"Failed to match consecutive patterns {patterns} in console log."
  )
