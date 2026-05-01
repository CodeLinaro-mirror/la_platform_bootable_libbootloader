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

.section .text.start, "ax", %progbits
.global _start
.hidden _start

_start:
    // For now we resuse the stack from previous stage. If it runs into issues,
    // we will need to set up our own stack.

    // Get current programmatic offset (runtime base of image data).
    // The `adr` instruction is PC relative and retrieves address of _start.
    adr x0, _start

    // Get current programmatic address of dynamic segment.
    // `_DYNAMIC` is provided by the linker and holds relative offsets to preferred base.
    adrp x1, _DYNAMIC
    add x1, x1, :lo12:_DYNAMIC

    // Call ApplyRelocation(program_base, dynamic_section) in relocation.cpp
    bl ApplyRelocation

    // Now the runtime is successfully relocated. Pass Execution to C code.
    bl kernel_main

spin:
    b spin
