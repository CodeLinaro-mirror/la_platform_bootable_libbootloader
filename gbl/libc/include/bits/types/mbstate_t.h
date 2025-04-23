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

#ifndef __STDLIB_MBSTATE_T_H__
#define __STDLIB_MBSTATE_T_H__

// Glibc uses this guard, rely on it to define mbstate_t only for non-std env.
#ifndef __mbstate_t_defined
#define __mbstate_t_defined 1

typedef void *mbstate_t;

#endif  // __mbstate_t_defined

#endif  // __STDLIB_MBSTATE_T_H__
