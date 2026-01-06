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

#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <uefi/efi.h>

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
  // Actual function return address.
  size_t return_addr;
} mcount_stack[MCOUNT_STACK_SIZE];
static size_t mcount_stack_top = 0;

// Flags to prevent re-entrant.
static bool in_mcount = false;

// TODO(b/473552136): Maximum system stack usage. Temporary for test.
static size_t peak_stack = 0;
__attribute__((no_instrument_function)) size_t get_peak_stack() {
  return peak_stack;
}

// mcount function entry.
__attribute__((no_instrument_function)) __attribute__((noinline)) void
mcount_func_entry(size_t entry_stack_addr) {
  // Don't proceed if re-entrant.
  if (in_mcount) {
    return;
  }

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

  // <function> -> mcount() -> mcount_func_entry()
  size_t func_return_addr = fr->parent->parent->return_address;
  // Override function return address to capture function exit.
  fr->parent->parent->return_address = (size_t)mcount_return_override;
  mcount_stack[mcount_stack_top++] =
      (struct McountStackEntry){func_return_addr};
  in_mcount = false;
  return;
}

// Function exit hook.
__attribute__((no_instrument_function)) size_t
mcount_func_exit(size_t exit_stack_addr) {
  // TODO(b/473552136): collect trace information such as timestamp, stack
  // snapshot.
  return mcount_stack[--mcount_stack_top].return_addr;
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

  // OEM firmware, i.e. u-boot, may not have unwind table enabled. Thus set this
  // frame as the end node.
  ((struct FrameRecord*)__builtin_frame_address(0U))->parent = NULL;

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
  // Start GBL on the new stack.
  return efi_main_switch_stack(image_handle, systab, (void*)gbl_stack_end);
}
