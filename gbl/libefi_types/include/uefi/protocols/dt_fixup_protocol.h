/*
 * Copyright (C) 2024-2025 The Android Open Source Project
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
 * SPDX-License-Identifier: Apache-2.0 OR BSD-2-Clause-Patent
 *
 * You may choose to use or redistribute this file under
 *  (a) the Apache License, Version 2.0, or
 *  (b) the BSD 2-Clause Patent license.
 *
 * Unless you expressly elect the BSD-2-Clause-Patent terms, the Apache-2.0
 * terms apply by default.
 */

// This is a protocol proposed by Heinrich Schuchardt and already being used by
// the Kernel UEFI stub.
// https://github.com/U-Boot-EFI/EFI_DT_FIXUP_PROTOCOL

#ifndef __EFI_DT_FIXUP_PROTOCOL_H__
#define __EFI_DT_FIXUP_PROTOCOL_H__

#include <uefi/types.h>

static const uint64_t EFI_DT_FIXUP_PROTOCOL_REVISION = 0x00010000;

typedef struct EfiDtFixupProtocol {
  uint64_t revision;
  EfiStatus (*fixup)(struct EfiDtFixupProtocol* self, /* in-out */ void* fdt,
                     /* in-out */ size_t* buffer_size);
} EfiDtFixupProtocol;

#endif  // __EFI_DT_FIXUP_PROTOCOL_H__
