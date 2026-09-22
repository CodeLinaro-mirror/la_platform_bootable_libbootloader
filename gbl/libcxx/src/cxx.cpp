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
 */

#include <stddef.h>
#include <stdlib.h>

#ifdef __GBL_LIBC_STUBS__
extern "C" [[noreturn]] void gbl_panic_from_c(const char* msg);
#endif

[[noreturn]] static void allocation_failed(const char* msg) {
#ifdef __GBL_LIBC_STUBS__
  gbl_panic_from_c(msg);
#else
  (void)msg;
  abort();
#endif
}

void* operator new(size_t size) {
  void* ptr = malloc(size);
  if (!ptr) {
    allocation_failed("new() failed to allocate memory");
  }
  return ptr;
}

void* operator new[](size_t size) {
  void* ptr = malloc(size);
  if (!ptr) {
    allocation_failed("new[]() failed to allocate memory");
  }
  return ptr;
}

void operator delete(void* ptr) noexcept { free(ptr); }

void operator delete[](void* ptr) noexcept { free(ptr); }

// For simplicity we always read the size from the allocation metadata, it's not
// worth the extra code complexity of using this arg to optimize.
void operator delete(void* ptr, size_t _size) noexcept { free(ptr); }

void operator delete[](void* ptr, size_t _size) noexcept { free(ptr); }
