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

#ifndef __FDT_INSTRUMENT_H__
#define __FDT_INSTRUMENT_H__

__attribute__((no_instrument_function)) const void* fdt_offset_ptr(
    const void* fdt, int offset, unsigned int len);

__attribute__((no_instrument_function)) unsigned int fdt_next_tag(
    const void* fdt, int startoffset, int* nextoffset);

#endif  // __FDT_INSTRUMENT_H__
