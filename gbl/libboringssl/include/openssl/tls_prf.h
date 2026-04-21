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
#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif

/*
 * CRYPTO_tls1_prf is needed because the official BoringSSL Rust library
 * (bssl-crypto), which GBL compiles directly, contains `tls12_prf.rs` that
 * references this symbol. Since GBL builds a baremetal libcrypto without SSL,
 * we stub/declare it here to satisfy bindgen without pulling in full TLS/SSL
 * protocol modules.
 */
OPENSSL_EXPORT int CRYPTO_tls1_prf(const EVP_MD* digest, uint8_t* out,
                                   size_t out_len, const uint8_t* secret,
                                   size_t secret_len, const uint8_t* label,
                                   size_t label_len, const uint8_t* seed1,
                                   size_t seed1_len, const uint8_t* seed2,
                                   size_t seed2_len);

#if defined(__cplusplus)
}  // extern C
#endif
