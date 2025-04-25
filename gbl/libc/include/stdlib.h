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
 */

#ifndef __STDLIB_H__
#define __STDLIB_H__

#include <gbl/defs.h>
#include <string.h>

__BEGIN_DECLS

// Following are definitions referenced by floating point APIs in clang C++
// headers.
//
// GBL does not use floating points. This is merely for getting compilation
// successful.
typedef struct {
  long int quot;
  long int rem;
} ldiv_t;

typedef struct {
  long long quot;
  long long rem;
} lldiv_t;

ldiv_t ldiv(long numer, long denom);
lldiv_t lldiv(long long numer, long long denom);

__attribute__((noreturn)) void abort();

__END_DECLS

#endif
