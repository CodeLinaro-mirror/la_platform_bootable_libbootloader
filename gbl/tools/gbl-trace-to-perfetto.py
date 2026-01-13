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
"""Convert GBL trace data to trace event format"""

# See http://docs/document/d/1CvAClvFfyA5R-PhYUmn5OOQtYMH4h6I0nSsKchNAySU/preview?tab=t.
# for details on format

import argparse
import json
import os
import pathlib
import re
import struct
import subprocess
import sys

SCRIPT_DIR = pathlib.Path(os.path.dirname(os.path.realpath(__file__)))

# Must be kept in sync with libtrace/include/gbl_trace.h
ENTRY_HEADER_FORMAT = "QQ"
ENTRY_HEADER_SIZE = struct.calcsize(ENTRY_HEADER_FORMAT)
FUNCTION_ENTRY = 0
FUNCTION_ENTRY_FORMAT = "QQQQ"
FUNCTION_EXIT = 1
FUNCTION_EXIT_FORMAT = "QQQQ"
HEAP_SNAPSHOT = 2
HEAP_SNAPSHOT_FORMAT = "Q"
META_FORMAT = "QQQQQQ"

FORMATS = {
    FUNCTION_ENTRY: FUNCTION_ENTRY_FORMAT,
    FUNCTION_EXIT: FUNCTION_EXIT_FORMAT,
    HEAP_SNAPSHOT: HEAP_SNAPSHOT_FORMAT,
}


# Find an llvm-symboilzer tool from prebuilts checked out in GBL repo.
def find_gbl_repo_llvm_symbolizer():
  try:
    aosp = SCRIPT_DIR.parents[4]
    clang_prebuilts = aosp / "prebuilts" / "clang" / "host" / "linux-x86"
    return next(clang_prebuilts.rglob("llvm-symbolizer"), None)
  except IndexError:
    return None


def parse_args():
  parser = argparse.ArgumentParser()
  parser.add_argument("trace", type=str, help="Path to the GBL trace file")
  parser.add_argument("bin", help="Path to the binary file.")
  parser.add_argument("out", help="output file")
  parser.add_argument("--llvm-symbolizer", help="Path to llvm-symbolizer")
  return parser.parse_args()


# Extracts a GBL trace entry
#
# Returns (entry type, ticks, <fields>, ending offset)
def parse_entry(
    trace,
    off,
):
  entry_type, tick = struct.unpack_from(ENTRY_HEADER_FORMAT, trace, off)
  off += ENTRY_HEADER_SIZE
  fmt = FORMATS[entry_type]
  sz = struct.calcsize(fmt)
  return (entry_type, tick, struct.unpack_from(fmt, trace, off), off + sz)


# Symbolizes the given set of addresses
def collect_symbols(addrs, efi_bin, llvm_symbolizer):
  # Removes null address if any.
  addrs.discard(0)
  addrs = [f"{addr:#x}" for addr in addrs]
  res = subprocess.run(
      [
          llvm_symbolizer,
          "-e",
          efi_bin,
          "-f",
          "-a",
          "--demangle",
          "--relative-address",
      ]
      + addrs,
      check=True,
      text=True,
      capture_output=True,
  ).stdout.strip()
  res = [v.split("\n") for v in re.split("\n\n", res)]
  return {int(v[0], 16): v[1:] for v in res}


def tick_to_micros(ts, freq):
  return ts * 1000 * 1000 / freq


def main():
  args = parse_args()
  llvm_symbolizer = (
      args.llvm_symbolizer
      or find_gbl_repo_llvm_symbolizer()
      or "llvm-symbolizer"
  )
  trace_bin = pathlib.Path(args.trace).read_bytes()

  # Checks metadata and potential truncation due to buffer limit
  magic, sz, overhead, freq, nr_func, nr_heap = struct.unpack_from(
      META_FORMAT, trace_bin
  )
  est_func_entry_size = nr_func * (
      struct.calcsize(FUNCTION_ENTRY_FORMAT) + ENTRY_HEADER_SIZE
  )
  est_func_exit_size = nr_func * (
      struct.calcsize(FUNCTION_EXIT_FORMAT) + ENTRY_HEADER_SIZE
  )
  est_heap_snapshot_size = nr_heap * (
      struct.calcsize(HEAP_SNAPSHOT_FORMAT) + ENTRY_HEADER_SIZE
  )
  est_full_size = (
      est_func_entry_size + est_func_exit_size + est_heap_snapshot_size
  )
  if est_full_size > sz:
    print(
        "Device omitted some traces due to buffer limit or unreturned"
        " functions."
    )
    print(
        "Estimated full trace size:"
        f" {est_full_size + struct.calcsize(META_FORMAT)} bytes. Got {sz}"
        " bytes."
    )

  print("Converting to trace event format...")
  trace_bin = trace_bin[struct.calcsize(META_FORMAT) :]

  # Parses all trace entries and collect addresses that need to be symbolized.
  entries = []
  addrs = set({})
  off = 0
  min_tick = 1 << 64
  max_tick = 0
  while off < len(trace_bin):
    progress_percent = off * 100 // len(trace_bin)
    print(f"\rCollecting raw traces {progress_percent}%    ", end="")
    type, tick, fields, off = parse_entry(trace_bin, off)
    min_tick = min(min_tick, tick)
    max_tick = max(max_tick, tick)
    if type == FUNCTION_ENTRY:
      # Function address, callsite address
      addrs.update([fields[0], fields[1]])
    entries.append((type, tick, fields))
  print("Done")

  # Display trace/platform info and params
  print(f"Tick frequency: {freq} Hz")
  print(
      f"Trace duration {tick_to_micros(max_tick - min_tick, freq)/1000:.2f}ms"
      f" ({max_tick - min_tick} ticks)"
  )

  # Collects symbols for all addresses.
  print(f"Symbolizing addresses...")
  syms = collect_symbols(addrs, args.bin, llvm_symbolizer)
  print("Done")

  # Converts to trace event format.
  traces = []
  # Dictionary for storing execution intervals, stack usages, entry/exit total count of each function
  func_stat = {"dur": {}, "stack": {}, "count": {}}
  # A stack for pairing function entries/exits to compute exec intervals etc.
  unmatched_entry = []

  # Generates function call, stack snapshot events
  for i, (type, tick, fields) in enumerate(entries, 1):
    # Show progress
    print(f"\rGenerating function events {i}/{len(entries)}   ", end="")
    ts = tick_to_micros(tick, freq)
    if type == FUNCTION_ENTRY:
      addr, callsite_addr, sys_stack, func_stack = fields
      # (# entry events, # exit events)
      func_stat["count"].setdefault(addr, [0, 0])
      func_stat["count"][addr][0] += 1
      v = func_stat["stack"].setdefault(addr, func_stack)
      func_stat["stack"][addr] = max(v, func_stack)

      # Get pretty-printed symbolized callsite
      if callsite_addr != 0:
        callsite = syms[callsite_addr]
        callsite = f"{callsite[0]} at {callsite[1]}\n" + "\n".join(
            f"  (inlined by) {callsite[i]} at {callsite[i+1]}"
            for i in range(2, len(callsite), 2)
        )
      else:
        # The case of tail call
        parent_addr = unmatched_entry[-1][1]
        callsite = (
            f"Tail call from {syms[parent_addr][0]} at {syms[parent_addr][1]}"
        )

      # Construct events
      traces.extend([
          # Duration begin event
          {
              "name": syms[addr][0],
              "cat": "function call",
              "ph": "B",
              "ts": ts,
              "pid": 0,
              "tid": 0,
              "args": {
                  "function address": f"{addr:#x}",
                  "function definition": syms[addr][1],
                  "call site address": f"{callsite_addr:#x}",
                  "call site": callsite,
                  "function stack usage": f"{func_stack} bytes",
              },
          },
          # Counter event for system stack usage
          {"name": "", "ph": "C", "ts": ts, "args": {"stack usage": sys_stack}},
      ])

      unmatched_entry.append((tick, addr))
    elif type == FUNCTION_EXIT:
      entry_tick, entry_func_addr = unmatched_entry.pop()
      addr, sys_stack, tracing_overhead, tracing_calls = fields
      assert (
          entry_func_addr == addr
      ), "Inconsistent function address for exit and entry"
      func_stat["count"][addr][1] += 1
      # Removes recursive calls from total time calculation.
      # Intervals are sorted.
      durs = func_stat["dur"].setdefault(addr, [])
      while len(durs) and durs[-1][0] >= entry_tick and durs[-1][1] <= tick:
        durs.pop()
      durs.append([
          entry_tick,  # entry tick
          tick,  # exit tick
          tracing_overhead,  # tracing time overhead
          tracing_calls,  # number of tracing calls.
      ])
      traces.extend([
          # Duration end event.
          {"ph": "E", "ts": ts, "pid": 0, "tid": 0},
          # Counter event for system stack usage
          {"name": "", "ph": "C", "ts": ts, "args": {"stack usage": sys_stack}},
      ])
    elif type == HEAP_SNAPSHOT:
      traces.append(
          # Counter event for system heap usage
          {"name": "", "ph": "C", "ts": ts, "args": {"heap usage": fields[0]}},
      )
  print("Done")

  # For function calls that didn't return, assume they return at `max_tick`.
  for t, addr in unmatched_entry:
    func_stat["dur"].setdefault(addr, []).append([t, max_tick, 0, 0])
  # Also add tracing overhead total, represent as 0 address
  func_stat["dur"].setdefault(0, []).append([0, overhead, 0, 0])

  # Draw a bar plot for total time consumed by each function.

  print(f"Generating function total time info...")
  # Sum the array [(entry tick, exit tick, overhead, tracing calls)] by column.
  dur_sums = [
      ([sum(v) for v in zip(*t)], addr)
      for (addr, t) in func_stat["dur"].items()
  ]
  # Maps to (duration without overhead, function address)
  dur_sums = [(v[1] - v[0] - v[2], v[2], v[3], addr) for (v, addr) in dur_sums]
  # Sort decreasingly.
  dur_sums.sort(reverse=True)
  display_tab_id = 1  # Group under new tab
  traces.append(
      {
          "name": "process_name",
          "ph": "M",
          "pid": display_tab_id,
          "args": {"name": "Function Total Time"},
      },
  )

  start = tick_to_micros(min_tick, freq)
  for i, (duration, overhead, tracing_calls, addr) in enumerate(
      dur_sums, display_tab_id
  ):
    percentage = f"{duration / (max_tick - min_tick) * 100:.2f}%"
    # Rounds to nano secs to prevent spurious overlap due to float precision,
    # which causes the timeline to be dropped by perfetto silently.
    duration = tick_to_micros(duration, freq) * 1000 // 1 / 1000
    overhead = tick_to_micros(overhead, freq) * 1000 // 1 / 1000
    # Plot a bar like: |time without overhead | overhead |
    traces.extend([
        # Each bar is plotted as a thread timeline so they align from the start.
        {
            "name": "thread_name",
            "ph": "M",
            "pid": display_tab_id,
            "tid": i,
            "args": {"name": f" {percentage} by #Function"},
        },
        # | time without overhead |
        {
            "name": syms[addr][0] if addr != 0 else "overhead",
            "cat": "total time",
            "ph": "X",
            "ts": start,
            "dur": duration,
            "pid": display_tab_id,
            "tid": i,
            "args": {
                "function addr": f"{addr:#x}" if addr != 0 else "NA",
                "function definition": syms[addr][1] if addr != 0 else "NA",
                "number of calls": func_stat["count"].get(addr, 1),
            },
        },
        # | overhead |
        {
            "name": "overhead",
            "cat": "total time",
            "ph": "X",
            "ts": start + duration,
            "dur": overhead,
            "pid": display_tab_id,
            "tid": i,
            "args": {"number of calls": tracing_calls},
        },
    ])

  # Draw a bar plot for stack consumption by each function

  print(f"Generating function stack usage info...")
  total_stack = [(v, addr) for (addr, v) in func_stat["stack"].items()]
  total_stack.sort(reverse=True)

  display_tab_id += 1
  traces.append(
      {
          "name": "process_name",
          "ph": "M",
          "pid": display_tab_id,
          "args": {"name": "Function Stack Usage"},
      },
  )

  # Each bar is plotted as a thread timeline, scale the unit so that they
  # display/align nicely with other timeline.
  scale = (max_tick - min_tick) / total_stack[0][0]
  for i, (stack, addr) in enumerate(total_stack, display_tab_id):
    virtual_duration = tick_to_micros(stack * scale, freq)
    traces.extend([
        {
            "name": "thread_name",
            "ph": "M",
            "pid": display_tab_id,
            "tid": i,
            "args": {"name": f"{stack} bytes by #Function"},
        },
        {
            "name": f"{syms[addr][0]}",
            "cat": "stack usage",
            "ph": "X",
            "ts": start,
            "dur": virtual_duration,
            "pid": display_tab_id,
            "tid": i,
            "args": {
                "function addr": f"{addr:#x}",
                "function definition": syms[addr][1],
            },
        },
    ])

  # Draw a bar plot for total number of calls of each function.
  # This is mainly for evaluating trace size contributed by each function.

  count = [(v[0], v[1], addr) for (addr, v) in func_stat["count"].items()]
  count.sort(reverse=True)

  display_tab_id += 1
  traces.append(
      {
          "name": "process_name",
          "ph": "M",
          "pid": display_tab_id,
          "args": {"name": "Count / % of Trace Size"},
      },
  )

  scale = (max_tick - min_tick) / count[0][0]
  for i, (c_entry, c_exit, addr) in enumerate(count, display_tab_id):
    size = c_entry * (16 + struct.calcsize(FUNCTION_ENTRY_FORMAT)) + c_exit * (
        16 + struct.calcsize(FUNCTION_EXIT_FORMAT)
    )
    sz_percent = size / len(trace_bin) * 100
    virtual_duration = tick_to_micros(c_entry * scale, freq)
    traces.extend([
        {
            "name": "thread_name",
            "ph": "M",
            "pid": display_tab_id,
            "tid": i,
            "args": {
                "name": f"{c_entry} times / {sz_percent:.2f}% by #Function"
            },
        },
        {
            "name": f"{syms[addr][0]}",
            "cat": "call count",
            "ph": "X",
            "ts": start,
            "dur": virtual_duration,
            "pid": display_tab_id,
            "tid": i,
            "args": {
                "function address": f"{addr:#x}",
                "function definition": syms[addr][1],
            },
        },
    ])

  print(f"Serializing to json...")
  serialized = json.dumps(traces)
  print(f"Writing to {args.out} ({len(serialized)} bytes)")
  pathlib.Path(args.out).write_text(serialized)
  print("Done")
  return 0


if __name__ == "__main__":
  sys.exit(main())
