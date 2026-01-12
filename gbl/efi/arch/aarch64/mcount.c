// Copyright 2026, The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#include <gbl_trace.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <uefi/efi.h>

#include "trace_buffer_size.h"

// Run efi_main using the given stack memory
extern EfiStatus efi_main_switch_stack(void* image_handle, void* systab,
                                       void* stack_end);
// Function return hook set by mcount
extern void mcount_return_override();

// A copy of system table.
static EfiSystemTable* gST = NULL;

#define EFI_PAGE_SIZE 4096

// Helper to Reset system.
__attribute__((no_instrument_function)) static void Reset(const uint16_t* msg) {
  if (!gST) {
    while (true) {
    }
  }
  gST->con_out->output_string(gST->con_out, (uint16_t*)msg);
  gST->runtime_services->reset_system(EFI_RESET_TYPE_COLD, 0, 0, NULL);
  while (true) {
  }
}

// Get CPU tick via aarch64 PMU (Performance Monitor Unit) counter.
// See ARMv8-A architecture reference for more detail.
__attribute__((no_instrument_function)) static uint64_t PmcTick() {
  uint64_t ticks = 0;
  asm volatile(
      "isb \n\t"
      "mrs %0, pmccntr_el0"
      : "=r"(ticks));
  return ticks;
}

// Enables PMU counter and resets its value.
// See ARMv8-A architecture reference for more detail.
__attribute__((no_instrument_function)) static void EnablePmc() {
  asm volatile(
      "isb \n\t"
      "mrs x0, pmcr_el0 \n\t"
      "orr x0, x0, #(1 << 2) \n\t"
      "orr x0, x0, #1 \n\t"
      "msr pmcr_el0, x0 \n\t"
      "mov x0, #(1 << 31) \n\t"
      "msr pmcntenset_el0, x0 \n\t" ::
          : "x0");
}

// The dedicated stack memory range for GBL.
static size_t gbl_stack_start = 0;
static size_t gbl_stack_end = 0;

// Checks if caller is on GBL stack.
__attribute__((no_instrument_function)) bool IsOnGblStack() {
  size_t fr = (size_t)__builtin_frame_address(0U);
  return fr < gbl_stack_end && fr >= gbl_stack_start;
}

// aarch64 frame record structure.
//
// This is the pair of x29,x30 stored on stack. Reference:
//
// * ARM64 ABI conventions.
// * arm-trusted-firmware.
struct FrameRecord {
  struct FrameRecord* parent;
  size_t return_address;
};

// Stack data structure for tracking real return address and other infos.
#define MCOUNT_STACK_SIZE 128
static struct McountStackEntry {
  // Function address (represented as mcount callsite)
  size_t func_addr;
  // Actual function return address.
  size_t return_addr;
  // Ticks due to mcount operations
  uint64_t mcount_overhead;
  // Number of mcount calls.
  uint64_t mcount_calls;
} mcount_stack[MCOUNT_STACK_SIZE];
static size_t mcount_stack_top = 0;

// Flags to prevent re-entrant.
static bool in_mcount = false;
// Base address of the loaded image.
static size_t image_base = 0;

// A placeholder trace buffer with empty size.
static GblTraceMetadata null_meta = {0, 0, 0, 0, 0, 0};

// Global buffer for storing trace data.
static struct TraceBuffer {
  uint8_t* buffer;
  size_t size;
} trace_buffer = {(uint8_t*)&null_meta, sizeof(null_meta)};

// Helper function for getting the metadata header.
// The helper assumes that `trace_buffer` either points to `null_meta` or
// allocated one by efi_main_tracing and thus is safe to call.
__attribute__((no_instrument_function)) static GblTraceMetadata* TraceMeta() {
  return (GblTraceMetadata*)trace_buffer.buffer;
}

// Helper for getting the current total trace size.
__attribute__((no_instrument_function)) static size_t CurrentTraceSize() {
  return TraceMeta()->size + sizeof(GblTraceMetadata);
}

// Reserves and allocates buffer for a new trace entry.
// The helper assumes that `trace_buffer` either points to `null_meta` or
// allocated one by efi_main_tracing and thus is always safe to call.
__attribute__((no_instrument_function)) static void* AllocateEntry(
    size_t size) {
  size_t new_total = CurrentTraceSize() + size;
  if (new_total < size || new_total > trace_buffer.size) {
    return NULL;
  }
  void* end = trace_buffer.buffer + CurrentTraceSize();
  TraceMeta()->size += size;
  return end;
}

// Flag to temporarily enable/disable mcount by user;
static bool enable_mcount = true;

// Enables or disables tracing
__attribute__((no_instrument_function)) void gbl_trace_set_enable(bool enable) {
  if (!IsOnGblStack()) {
    Reset(L"Trace: gbl_trace_set_enable() is not called on GBL stack");
  }
  enable_mcount = enable;
}

// Enables or disables tracing
__attribute__((no_instrument_function)) bool gbl_trace_get_enable() {
  return enable_mcount;
}

// Adds a heap snapshot event
__attribute__((no_instrument_function)) void gbl_trace_add_heap_snapshot(
    size_t total) {
  if (!IsOnGblStack() || !enable_mcount) {
    return;
  }

  GBlTraceHeapSnapshot* entry = AllocateEntry(sizeof(GBlTraceHeapSnapshot));
  TraceMeta()->heap_snapshot_events++;
  if (entry) {
    *entry = (GBlTraceHeapSnapshot){{GBL_TRACE_TYPE_HEAP_SNAPSHOT, PmcTick()},
                                    total};
  }
}

// Leaks and returns trace buffer to caller.
__attribute__((no_instrument_function)) void _gbl_trace_take_buffer(
    void** out, size_t* out_size, size_t* out_data_size) {
  if (!IsOnGblStack()) {
    Reset(L"Trace: take_trace_data() is not called on GBL stack");
  } else if (trace_buffer.buffer == (uint8_t*)&null_meta) {
    // Already taken.
    *out = NULL;
    *out_size = 0;
    *out_data_size = 0;
    return;
  }

  *out = trace_buffer.buffer;
  *out_size = trace_buffer.size;
  *out_data_size = CurrentTraceSize();
  trace_buffer = (struct TraceBuffer){(uint8_t*)&null_meta, sizeof(null_meta)};
}

// TODO(b/473552136): Maximum system stack usage. Temporary for test.
static size_t peak_stack = 0;
__attribute__((no_instrument_function)) size_t get_peak_stack() {
  return peak_stack;
}

// mcount function entry.
__attribute__((no_instrument_function)) __attribute__((noinline)) void
mcount_func_entry(size_t entry_stack_addr) {
  // Don't proceed if re-entrant or disabled
  if (in_mcount || !enable_mcount) {
    return;
  }

  uint64_t start = PmcTick();

  // Don't proceed if we are not on GBL stack. This could mean we are running
  // from different thread.
  if (!IsOnGblStack()) {
    return;
  }

  // If we run out of mcount_stack space, skips tracing.
  if (mcount_stack_top >= MCOUNT_STACK_SIZE) {
    return;
  }

  // <function> -> mcount() -> mcount_func_entry()
  struct FrameRecord* fr = __builtin_frame_address(0U);
  // Checks we have well formed unwind table up to the caller of the function.
  if (!(fr && fr->parent && fr->parent->parent)) {
    return;
  }

  in_mcount = true;

  // TODO(b/473552136): collect trace information such as timestamp, stack
  // snapshot.
  size_t stack = gbl_stack_end - entry_stack_addr;
  if (stack > peak_stack) {
    peak_stack = stack;
  }

  GblTraceFunctionEntry* entry = AllocateEntry(sizeof(GblTraceFunctionEntry));
  TraceMeta()->function_entry_events++;
  if (entry) {
    // <function> -> mcount() -> mcount_func_entry()
    size_t func_return_addr = fr->parent->parent->return_address;
    // Override function return address to capture function exit.
    size_t func_addr = fr->parent->return_address - 4 - image_base;
    *entry = (GblTraceFunctionEntry){
        {GBL_TRACE_TYPE_FUNCTION_ENTRY, start},
        func_addr,                                        // function address,
        func_return_addr - image_base - 4,                // callsite
        gbl_stack_end - entry_stack_addr,                 // sys stack snapshot
        (size_t)fr->parent->parent - (size_t)fr->parent,  // local stack used
    };
    fr->parent->parent->return_address = (size_t)mcount_return_override;
    mcount_stack[mcount_stack_top++] = (struct McountStackEntry){
        func_addr, func_return_addr, PmcTick() - start, 1};
  }
  TraceMeta()->tracing_overhead += PmcTick() - start;
  in_mcount = false;
  return;
}

// Function exit hook.
__attribute__((no_instrument_function)) size_t
mcount_func_exit(size_t exit_stack_addr) {
  // Prevents further entry/exit event.
  in_mcount = true;
  uint64_t start = PmcTick();
  struct McountStackEntry entry = mcount_stack[--mcount_stack_top];
  GblTraceFunctionExit* exit = AllocateEntry(sizeof(GblTraceFunctionExit));
  if (exit) {
    *exit = (GblTraceFunctionExit){
        {GBL_TRACE_TYPE_FUNCTION_EXIT, start},
        entry.func_addr,
        gbl_stack_end - exit_stack_addr,
        entry.mcount_overhead,
        entry.mcount_calls,
    };
  }
  // Accumulate mcount overhead to its caller if exists.
  if (mcount_stack_top) {
    mcount_stack[mcount_stack_top - 1].mcount_overhead +=
        entry.mcount_overhead + PmcTick() - start;
    mcount_stack[mcount_stack_top - 1].mcount_calls += entry.mcount_calls;
  }
  TraceMeta()->tracing_overhead += PmcTick() - start;
  in_mcount = false;
  return entry.return_addr;
}

// Helper for allocating pages.
__attribute__((no_instrument_function)) static void* AllocPage(
    EfiSystemTable* st, size_t pages) {
  EfiPhysicalAddr out = 0;
  if (st->boot_services->allocate_pages(EFI_ALLOCATOR_TYPE_ALLOCATE_ANY_PAGES,
                                        EFI_MEMORY_TYPE_LOADER_DATA, pages,
                                        &out) != EFI_STATUS_SUCCESS) {
    Reset(L"Trace: failed to allocate pages\n");
  }
  return (void*)out;
}

// Top level efi_main entry that allocates separate stack and setup mcount.
__attribute__((no_instrument_function)) EfiStatus
efi_main_tracing(void* image_handle, EfiSystemTable* systab) {
  gST = systab;

  EnablePmc();

  // PMC tick frequency is not a fixed value and varies as CPU frequency.
  // Measures the frequency at runtime and assume it doesn't change throughout
  // GBL.
  size_t tick = PmcTick();
  // This can also use aarch64 generic timer in case stall() is not implemented.
  gST->boot_services->stall(1000 * 1000);  // 1 sec
  uint64_t pmc_freq = PmcTick() - tick;

  // OEM firmware, i.e. u-boot, may not have unwind table enabled. Thus set this
  // frame as the end node.
  ((struct FrameRecord*)__builtin_frame_address(0U))->parent = NULL;

  // Get image base address for computing relative address of functions.
  EfiGuid loaded_image_protocol_guid = {
      0x5B1B31A1,
      0x9562,
      0x11d2,
      {0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B}};
  EfiLoadedImageProtocol* loaded_image = NULL;
  EfiStatus status = systab->boot_services->handle_protocol(
      image_handle, &loaded_image_protocol_guid, (void**)&loaded_image);
  if (status != EFI_STATUS_SUCCESS) {
    return status;
  }
  image_base = (size_t)loaded_image->image_base;

  // Allocate dedicated stack for GBL. The main purposes are:
  //
  // 1. A way to identify GBL main thread.
  // 2. Deterministic stack size/range for various analysis.
  //
  // Size is empirically chosen, enough for both release/debug build at the time
  // of writing.
  const size_t stack_size = 4 * 1024 * 1024;
  gbl_stack_start = (size_t)AllocPage(systab, stack_size / EFI_PAGE_SIZE);
  gbl_stack_end = gbl_stack_start + stack_size;

  // Allocate trace storage buffer.
  // TODO(b/473552136): Make trace buffer size configurable
  const size_t trace_buffer_size = TRACE_BUFFER_SIZE_MB * 1024 * 1024;
  trace_buffer.buffer = AllocPage(systab, trace_buffer_size / EFI_PAGE_SIZE);
  trace_buffer.size = trace_buffer_size;
  memset(TraceMeta(), 0, sizeof(GblTraceMetadata));
  TraceMeta()->magic = GBL_TRACE_MAGIC;
  TraceMeta()->timestamp_frequency = pmc_freq;

  // Start GBL on the new stack.
  return efi_main_switch_stack(image_handle, systab, (void*)gbl_stack_end);
}
