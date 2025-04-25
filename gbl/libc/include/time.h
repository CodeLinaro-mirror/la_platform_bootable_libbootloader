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

#ifndef __GBL_TIME_H__
#define __GBL_TIME_H__

// Definition required by clang C++ headers.
typedef long long time_t;

struct tm {
  int tm_sec;   /* Seconds          [0, 60] */
  int tm_min;   /* Minutes          [0, 59] */
  int tm_hour;  /* Hour             [0, 23] */
  int tm_mday;  /* Day of the month [1, 31] */
  int tm_mon;   /* Month            [0, 11]  (January = 0) */
  int tm_year;  /* Year minus 1900 */
  int tm_wday;  /* Day of the week  [0, 6]   (Sunday = 0) */
  int tm_yday;  /* Day of the year  [0, 365] (Jan/01 = 0) */
  int tm_isdst; /* Daylight savings flag */

  long tm_gmtoff;      /* Seconds East of UTC */
  const char* tm_zone; /* Timezone abbreviation */
};

time_t time(time_t* timer);

#endif  //__GBL_TIME_H__
