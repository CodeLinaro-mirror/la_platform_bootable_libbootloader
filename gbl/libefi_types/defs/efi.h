/*
 * Copyright (C) 2023 The Android Open Source Project
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

#ifndef __EFI_H__
#define __EFI_H__

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "boot_service.h"
#include "gbl_efi_common.h"
#include "protocols/block_io2_protocol.h"
#include "protocols/block_io_protocol.h"
#include "protocols/device_path_protocol.h"
#include "protocols/dt_fixup_protocol.h"
#include "protocols/erase_block_protocol.h"
#include "protocols/gbl_efi_ab_slot_protocol.h"
#include "protocols/gbl_efi_avb_protocol.h"
#include "protocols/gbl_efi_avf_protocol.h"
#include "protocols/gbl_efi_boot_memory_protocol.h"
#include "protocols/gbl_efi_debug_protocol.h"
#include "protocols/gbl_efi_fastboot_protocol.h"
#include "protocols/gbl_efi_fastboot_transport.h"
#include "protocols/gbl_efi_image_loading_protocol.h"
#include "protocols/gbl_efi_os_configuration_protocol.h"
#include "protocols/hash2_protocol.h"
#include "protocols/loaded_image_protocol.h"
#include "protocols/random_number_generator_protocol.h"
#include "protocols/riscv_efi_boot_protocol.h"
#include "protocols/service_binding_protocol.h"
#include "protocols/simple_network_protocol.h"
#include "protocols/simple_text_input_protocol.h"
#include "protocols/simple_text_output_protocol.h"
#include "protocols/timestamp.h"
#include "system_table.h"
#include "types.h"

#endif  // __EFI_H__
