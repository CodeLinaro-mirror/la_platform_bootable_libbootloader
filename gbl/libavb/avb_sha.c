/*
 * Copyright (C) 2025 The Android Open Source Project
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
#include "libavb/avb_sha.h"

#include "efi_sha_c.h"
#include "libavb/avb_crypto_ops_impl.h"

void avb_sha256_init(AvbSHA256Ctx* ctx) {
  void* hasher = efi_sha256_init();
  if (hasher) {
    ctx->impl.tag = AVB_HASH_EFI;
    ctx->impl.state.efi_hasher = hasher;
  } else {
    ctx->impl.tag = AVB_HASH_SSL;
    SHA256_Init(&ctx->impl.state.ssl_ctx);
  }
}

void avb_sha256_update(AvbSHA256Ctx* ctx, const uint8_t* data, size_t len) {
  if (ctx->impl.tag == AVB_HASH_EFI) {
    efi_sha256_update(ctx->impl.state.efi_hasher, data, len);
  } else if (ctx->impl.tag == AVB_HASH_SSL) {
    SHA256_Update(&ctx->impl.state.ssl_ctx, data, len);
  } else {
    avb_abort();
  }
}

uint8_t* avb_sha256_final(AvbSHA256Ctx* ctx) {
  if (ctx->impl.tag == AVB_HASH_EFI) {
    efi_sha256_final(&ctx->impl.state.efi_hasher, ctx->buf);
  } else if (ctx->impl.tag == AVB_HASH_SSL) {
    SHA256_Final(ctx->buf, &ctx->impl.state.ssl_ctx);
  } else {
    avb_abort();
  }

  ctx->impl.tag = AVB_HASH_UNKNOWN;
  return ctx->buf;
}

void avb_sha512_init(AvbSHA512Ctx* ctx) {
  void* hasher = efi_sha512_init();
  if (hasher) {
    ctx->impl.tag = AVB_HASH_EFI;
    ctx->impl.state.efi_hasher = hasher;
  } else {
    ctx->impl.tag = AVB_HASH_SSL;
    SHA512_Init(&ctx->impl.state.ssl_ctx);
  }
}

void avb_sha512_update(AvbSHA512Ctx* ctx, const uint8_t* data, size_t len) {
  if (ctx->impl.tag == AVB_HASH_EFI) {
    efi_sha512_update(ctx->impl.state.efi_hasher, data, len);
  } else if (ctx->impl.tag == AVB_HASH_SSL) {
    SHA512_Update(&ctx->impl.state.ssl_ctx, data, len);
  } else {
    avb_abort();
  }
}

uint8_t* avb_sha512_final(AvbSHA512Ctx* ctx) {
  if (ctx->impl.tag == AVB_HASH_EFI) {
    efi_sha512_final(&ctx->impl.state.efi_hasher, ctx->buf);
  } else if (ctx->impl.tag == AVB_HASH_SSL) {
    SHA512_Final(ctx->buf, &ctx->impl.state.ssl_ctx);
  } else {
    avb_abort();
  }

  ctx->impl.tag = AVB_HASH_UNKNOWN;
  return ctx->buf;
}
