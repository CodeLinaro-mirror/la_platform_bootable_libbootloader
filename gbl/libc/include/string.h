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

#ifndef __STDLIB_STRING_H__
#define __STDLIB_STRING_H__

#include <gbl/defs.h>
#include <stddef.h>

__BEGIN_DECLS

size_t strlen(const char *str);
void *memchr(const void *ptr, int ch, size_t count);
int memcmp(const void *ptr1, const void *ptr2, size_t num);
void *memset(void *destination, int c, size_t num);
void *memcpy(void *destination, const void *source, size_t num);
void *memmove(void *destination, const void *source, size_t num);
char *strrchr(const char *str, int c);
char *strchr(const char *str, int c);
size_t strnlen(const char *s, size_t maxlen);
int strcmp(const char *s1, const char *s2);
int strncmp(const char *s1, const char *s2, size_t n);
unsigned long int strtoul(const char *s, char **endptr, int base);

__END_DECLS
#endif
