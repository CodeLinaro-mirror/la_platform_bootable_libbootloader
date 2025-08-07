/*
 * Copyright (C) 2024 The Android Open Source Project
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

// This is a custom protocol introduced by GBL.
// See gbl/docs/gbl_efi_avb_protocol.md for details.

#ifndef __GBL_AVB_PROTOCOL_H__
#define __GBL_AVB_PROTOCOL_H__

#include "types.h"

const uint64_t GBL_EFI_AVB_PROTOCOL_REVISION = 0x00000001;

// Os boot state color.
//
// https://source.android.com/docs/security/features/verifiedboot/boot-flow#communicating-verified-boot-state-to-users
typedef enum GBL_EFI_AVB_BOOT_STATE_COLOR {
  GREEN,
  YELLOW,
  ORANGE,
  RED_EIO,
  RED,
} GblEfiAvbBootStateColor;

// Vbmeta key validation status.
//
// https://source.android.com/docs/security/features/verifiedboot/boot-flow#locked-devices-with-custom-root-of-trust
typedef enum GBL_EFI_AVB_KEY_VALIDATION_STATUS {
  VALID,
  VALID_CUSTOM_KEY,
  INVALID,
} GblEfiAvbKeyValidationStatus;

typedef struct {
  // GblEfiAvbBootStateColor
  uint32_t color;

  // Pointer to nul-terminated ASCII hex digest calculated by libavb. May be
  // null in case of verification failed (RED boot state color).
  const char8_t* digest;

  // Pointers to nul-terminated os versions and security_patches for different
  // boot components. NULL is provided in case value isn't presented in the boot
  // artifacts or fatal AVB failure.
  // https://source.android.com/docs/core/architecture/bootloader/version-info-avb
  const char8_t* boot_version;
  const char8_t* boot_security_patch;
  const char8_t* system_version;
  const char8_t* system_security_patch;
  const char8_t* vendor_version;
  const char8_t* vendor_security_patch;
} GblEfiAvbVerificationResult;

typedef struct {
  // On input - `base_name` buffer size
  // On output - actual `base_name` length
  size_t base_name_len;
  char8_t* base_name;
} GblEfiAvbPartition;

typedef struct GblEfiAvbProtocol {
  // Revision of the protocol supported.
  // Currently must contain 0x00000002000000000
  uint64_t revision;

  EfiStatus (*read_partitions_to_verify)(
      struct GblEfiAvbProtocol* self,
      /* in-out */ size_t* num_partitions,
      /* in-out */ GblEfiAvbPartition* partitions);

  EfiStatus (*read_is_dm_verity_error)(struct GblEfiAvbProtocol* self,
                                       /* out */ bool* is_dm_verity_error);

  EfiStatus (*validate_vbmeta_public_key)(
      struct GblEfiAvbProtocol* self,
      /* in */ const uint8_t* public_key_data,
      /* in */ size_t public_key_length,
      /* in */ const uint8_t* public_key_metadata,
      /* in */ size_t public_key_metadata_length,
      /* out GblEfiAvbKeyValidationStatus */ uint32_t* validation_status);

  EfiStatus (*read_is_device_unlocked)(struct GblEfiAvbProtocol* self,
                                       /* out */ bool* is_unlocked);

  EfiStatus (*read_rollback_index)(struct GblEfiAvbProtocol* self,
                                   /* in */ size_t index_location,
                                   /* out */ uint64_t* rollback_index);

  EfiStatus (*write_rollback_index)(struct GblEfiAvbProtocol* self,
                                    /* in */ size_t index_location,
                                    /* in */ uint64_t rollback_index);

  EfiStatus (*read_persistent_value)(struct GblEfiAvbProtocol* self,
                                     /* in */ const char8_t* name,
                                     /* out */ uint8_t* value,
                                     /* in-out */ size_t* value_size);

  EfiStatus (*write_persistent_value)(struct GblEfiAvbProtocol* self,
                                      /* in */ const char8_t* name,
                                      /* in */ const uint8_t* value,
                                      /* in */ size_t value_size);

  EfiStatus (*handle_verification_result)(
      struct GblEfiAvbProtocol* self,
      /* in */ const GblEfiAvbVerificationResult* result);

} GblEfiAvbProtocol;

#endif  //__GBL_AVB_PROTOCOL_H__
