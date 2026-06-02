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
 */

#ifndef __STDLIB_STDIO_H__
#define __STDLIB_STDIO_H__

#include <stdarg.h>
#include <stddef.h>

#include <gbl/defs.h>

__BEGIN_DECLS

// Required by LLVM libc++ char_traits.h pulled in by
// boringssl/include/openssl/span.h.
//
// Related bug: https://github.com/llvm/llvm-project/issues/85158
#define EOF (-1)

// Required by LLVM libc++ char_traits.h pulled in by
// boringssl/include/openssl/span.h.
//
// Related bug: https://github.com/llvm/llvm-project/issues/85335
int remove(const char *filename);

// Need to compile boringssl/include/openssl/err.h, but never getting used.
typedef int FILE;
extern FILE *stdin;
extern FILE *stdout;
extern FILE *stderr;

int fputs(const char *str, FILE *stream);
void perror(const char *s);

int snprintf(char *str, size_t size, const char *format, ...);
int vsnprintf(char *str, size_t size, const char *format, va_list ap);
int fprintf(FILE *stream, const char *format, ...);
int vasprintf(char **strp, const char *fmt, va_list ap);
int vsscanf(const char *str, const char *format, va_list ap);
int sscanf(const char *str, const char *format, ...);

// Stubs for BoringSSL
int fclose(FILE *fp);
size_t fread(void *ptr, size_t size, size_t nmemb, FILE *stream);
size_t fwrite(const void *ptr, size_t size, size_t nmemb, FILE *stream);
int fseek(FILE *stream, long offset, int whence);
long ftell(FILE *stream);
int feof(FILE *stream);
int ferror(FILE *stream);
int fflush(FILE *stream);
char *fgets(char *s, int size, FILE *stream);

__END_DECLS

#endif  // __STDLIB_STDIO_H__
