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
 */

#ifndef GBL_NASM_CONFIG_H_
#define GBL_NASM_CONFIG_H_

// Include NASM's upstream platform configuration header.
#if defined(__APPLE__)
#include "config/config-mac.h"
#elif defined(_WIN32)
#include "config/msvc.h"
#else
#include "config/config-linux.h"
#endif

// canonicalize_file_name is a GNU extension not available on macOS or musl libc.
#undef HAVE_CANONICALIZE_FILE_NAME

#endif  // GBL_NASM_CONFIG_H_
