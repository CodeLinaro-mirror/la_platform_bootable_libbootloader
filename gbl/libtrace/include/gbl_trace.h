/*
 * Copyright (C) 2026 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 */

// The file contains wire format of GBL trace data.

#ifndef __GBL_TRACE_H__
#define __GBL_TRACE_H__

#include <stdint.h>

#define GBL_TRACE_MAGIC 0x0641dac6bd9d2ea3
#define GBL_TRACE_TYPE_FUNCTION_ENTRY 0
#define GBL_TRACE_TYPE_FUNCTION_EXIT 1
#define GBL_TRACE_TYPE_HEAP_SNAPSHOT 2

// Metadata for the trace.
typedef struct GblTraceMetadata {
  // `GBL_TRACE_MAGIC`.
  uint64_t magic;
  // Total size excluding metadata.
  uint64_t size;
  // Total time overhead due to tracing
  uint64_t tracing_overhead;
  // Frequency of timerstamp tick in the trace.
  uint64_t timestamp_frequency;
  // Number of function entry captured, used for estimating truncated size.
  uint64_t function_entry_events;
  // Number of heap snapshot events, used for estimating truncated size.
  uint64_t heap_snapshot_events;
} GblTraceMetadata;

typedef struct GblTraceEntryHeader {
  // Type code.
  uint64_t type_code;
  // Timestamp of entry.
  uint64_t timestamp;
} GblTraceEntryHeader;

// Function entry trace.
typedef struct GblTraceFunctionEntry {
  // Type code should always be `GBL_TRACE_TYPE_FUNCTION_ENTRY`.
  GblTraceEntryHeader header;
  // Address of function.
  uint64_t addr;
  // Address of call site.
  uint64_t call_site_addr;
  // Snapshot of system stack usage.
  uint64_t sys_stack_snapshot;
  // Stack used by this function.
  uint64_t func_stack_usage;
} GblTraceFunctionEntry;

// Function exit trace.
typedef struct GblTraceFunctionExit {
  // Type code should always be `GBL_TRACE_TYPE_FUNCTION_EXIT`.
  GblTraceEntryHeader header;
  // Address of function.
  uint64_t addr;
  // Snapshot of system stack usage upon exit.
  uint64_t stack_snapshot;
  // overhead due to tracing for the entire function call
  uint64_t tracing_overhead;
  // Number of tracing calls.
  uint64_t tracing_calls;
} GblTraceFunctionExit;

typedef struct GBlTraceHeapSnapshot {
  // Type code should always be `GBL_TRACE_TYPE_HEAP_SNAPSHOT`.
  GblTraceEntryHeader header;
  // Total heap used.
  uint64_t total_heap_usage;
} GBlTraceHeapSnapshot;

#endif  // __GBL_TRACE_H__
