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

#ifndef __GBL_PROTOCOL_UTILS_H__
#define __GBL_PROTOCOL_UTILS_H__

#define GBL_PROTOCOL_MAJOR_REV(x) (((x) >> 16) & 0xFFFF)
#define GBL_PROTOCOL_MINOR_REV(x) ((x) & 0xFFFF)

#define GBL_PROTOCOL_REVISION(major, minor) \
  ((((major) & 0xFFFF) << 16) | ((minor) & 0xFFFF))

#endif  // __GBL_PROTOCOL_UTILS_H__
