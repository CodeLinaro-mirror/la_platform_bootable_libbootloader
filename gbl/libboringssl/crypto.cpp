// Copyright 2025, The Android Open Source Project
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

// Note: we cannot use boringssl's "crypto/crypto.cc" to define OPENSSL_init_cpuid
//       et al. because it assumes more OS support than will be available in UEFI.
//       Just redefine the symbols we actually need and let the linker take care
//       of the rest.

#include <internal.h>

namespace bssl {

__attribute__((visibility("hidden"))) uint32_t OPENSSL_armcap_P = 0;

uint32_t OPENSSL_get_armcap(void) {
  OPENSSL_init_cpuid();
  return OPENSSL_armcap_P;
}

static CRYPTO_once_t once = CRYPTO_ONCE_INIT;

void OPENSSL_init_cpuid(void) { CRYPTO_once(&once, OPENSSL_cpuid_setup); }

}  // namespace bssl
