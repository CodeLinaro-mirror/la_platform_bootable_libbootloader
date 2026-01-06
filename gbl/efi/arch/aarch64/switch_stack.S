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

.global efi_main_switch_stack

/* -----------------------------------------------------
 * EfiStatus efi_main_switch_stack(void* image_handle, void* systab, void* stack_end)
 *
 * Use the given memory as stack buffer and call efi_main.
 * -----------------------------------------------------
 */
efi_main_switch_stack:
    paciasp
    sub sp, sp, #0x20
    stp x29, x30, [sp]
    stp x27, x28, [sp, #0x10]
    mov x28, sp
    mov sp, x2
    bl efi_main
    mov sp, x28
    ldp x29, x30, [sp]
    ldp x27, x28, [sp, #0x10]
    add sp, sp, #0x20
    autiasp
    ret
