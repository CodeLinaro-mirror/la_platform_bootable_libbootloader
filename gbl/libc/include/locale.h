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

#ifndef __LOCALE_H__
#define __LOCALE_H__

#include <ctype.h>
#include <gbl/defs.h>

__BEGIN_DECLS

struct lconv {
  char* decimal_point;
  char* thousands_sep;
  char* grouping;
  char* int_curr_symbol;
  char* currency_symbol;
  char* mon_decimal_point;
  char* mon_thousands_sep;
  char* mon_grouping;
  char* positive_sign;
  char* negative_sign;
  char int_frac_digits;
  char frac_digits;
  char p_cs_precedes;
  char p_sep_by_space;
  char n_cs_precedes;
  char n_sep_by_space;
  char p_sign_posn;
  char n_sign_posn;
  char int_p_cs_precedes;
  char int_p_sep_by_space;
  char int_n_cs_precedes;
  char int_n_sep_by_space;
  char int_p_sign_posn;
  char int_n_sign_posn;
};

#define LC_ALL 0
#define LC_COLLATE 1
#define LC_CTYPE 2
#define LC_MONETARY 3
#define LC_NUMERIC 4
#define LC_TIME 5

#define LC_COLLATE_MASK (1 << LC_COLLATE)
#define LC_CTYPE_MASK (1 << LC_CTYPE)
#define LC_MONETARY_MASK (1 << LC_MONETARY)
#define LC_NUMERIC_MASK (1 << LC_NUMERIC)
#define LC_TIME_MASK (1 << LC_TIME)
#define LC_MESSAGES_MASK (1 << 6)
#define LC_ALL_MASK                                                       \
  (LC_COLLATE_MASK | LC_CTYPE_MASK | LC_MONETARY_MASK | LC_NUMERIC_MASK | \
   LC_TIME_MASK | LC_MESSAGES_MASK)

struct lconv* localeconv(void);
typedef void* _locale_t;
typedef _locale_t locale_t;
locale_t uselocale(locale_t newloc);

float strtof_l(const char* nptr, char** endptr, locale_t loc);
long double strtold_l(const char* nptr, char** endptr, locale_t loc);
double strtod_l(const char* nptr, char** endptr, locale_t loc);
long long strtoll_l(const char* nptr, char** endptr, int base, locale_t loc);
unsigned long long strtoull_l(const char* nptr, char** endptr, int base,
                              locale_t loc);

static inline int isalnum_l(int c, locale_t l) { return isalnum(c); }
static inline int isalpha_l(int c, locale_t l) { return isalpha(c); }
static inline int isblank_l(int c, locale_t l) { return isblank(c); }
static inline int iscntrl_l(int c, locale_t l) { return iscntrl(c); }
static inline int isdigit_l(int c, locale_t l) { return isdigit(c); }
static inline int isgraph_l(int c, locale_t l) { return isgraph(c); }
static inline int islower_l(int c, locale_t l) { return islower(c); }
static inline int isprint_l(int c, locale_t l) { return isprint(c); }
static inline int ispunct_l(int c, locale_t l) { return ispunct(c); }
static inline int isspace_l(int c, locale_t l) { return isspace(c); }
static inline int isupper_l(int c, locale_t l) { return isupper(c); }
static inline int isxdigit_l(int c, locale_t l) { return isxdigit(c); }

static inline int _isdigit_l(int c, _locale_t l) { return isdigit(c); }
static inline int _isxdigit_l(int c, _locale_t l) { return isxdigit(c); }

static inline int tolower_l(int c, locale_t l) { return tolower(c); }
static inline int toupper_l(int c, locale_t l) { return toupper(c); }

__END_DECLS

#endif  // __LOCALE_H__
