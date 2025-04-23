// Copyright 2025, The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Subset of boringssl crypto/fipsmodule/bcm.cc to allow compiling the required
// fips modules only.

#include "crypto/fipsmodule/digest/digest.cc.inc"
#include "crypto/fipsmodule/digest/digests.cc.inc"
#include "crypto/fipsmodule/hkdf/hkdf.cc.inc"
#include "crypto/fipsmodule/hmac/hmac.cc.inc"
#include "crypto/fipsmodule/sha/sha1.cc.inc"
#include "crypto/fipsmodule/sha/sha256.cc.inc"
#include "crypto/fipsmodule/sha/sha512.cc.inc"