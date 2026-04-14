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

#ifndef __NL_TYPES_H__
#define __NL_TYPES_H__

#include <gbl/defs.h>

__BEGIN_DECLS

typedef void* nl_catd;
#define NL_SETD 1
#define NL_CAT_LOCALE 1

nl_catd catopen(const char* name, int oflag);
char* catgets(nl_catd catd, int set_id, int msg_id, const char* s);
int catclose(nl_catd catd);

__END_DECLS

#endif  // __NL_TYPES_H__
