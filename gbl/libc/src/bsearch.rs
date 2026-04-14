// Copyright 2026, The Android Open Source Project
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

use core::ffi::{c_int, c_void};

/// C-compatible binary search using Rust's `core::slice::binary_search_by`.
///
/// # Safety
///
/// * `base` must point to an array of `nmemb` elements, each of `size` bytes.
/// * `key` must point to the object being searched for.
/// * `cmp` must be a valid function pointer that can cmp the key with elements in the array.
/// * The memory range `base` to `base + nmemb * size` must be valid for reads.
#[no_mangle]
pub unsafe extern "C" fn bsearch_rust(
    key: *const c_void,
    base: *const c_void,
    nmemb: usize,
    size: usize,
    cmp: unsafe extern "C" fn(*const c_void, *const c_void) -> c_int,
) -> *mut c_void {
    if nmemb == 0 || size == 0 {
        return core::ptr::null_mut();
    }
    let base = base as *const u8;

    // Create a dummy slice of length `nmemb` starting at `base`.
    // We only use the references passed to the closure to calculate indices,
    // we never actually dereference them to read byte values.
    // SAFETY: Since it covers the first `nmemb` bytes, and the actual data covers
    // `nmemb * size` bytes, this slice is entirely within the valid memory region
    // (since `size >= 1`).
    let dummy_slice: &[u8] = unsafe { core::slice::from_raw_parts(base, nmemb) };

    let result = dummy_slice.binary_search_by(|probe| {
        // Calculate the index by finding the offset of the probed reference from the base.
        // SAFETY: `probe` is a reference within `dummy_slice`, which is derived from `base`.
        // Both pointers are within the same allocated object.
        let idx = unsafe { (probe as *const u8).offset_from(base) } as usize;

        // Calculate the actual pointer to the element of size `size`.
        // SAFETY: `base` is guarantied to point to valid array of size nmemb*size, per function
        // safety. `idx` is calculated as valid offset less than nmemb, as probe is index within
        // dummy_slice.
        let element_ptr = unsafe { base.add(idx * size) };

        // Call the C comparator.
        // SAFETY: `cmp` is assumed to be a valid function pointer.
        // `key` is pointing to valid object as per function safety.
        // `element_ptr` is guarantied to point to valid array element as per safety requirements.
        let result = unsafe { cmp(key, element_ptr as *const c_void) };

        if result < 0 {
            core::cmp::Ordering::Greater
        } else if result > 0 {
            core::cmp::Ordering::Less
        } else {
            core::cmp::Ordering::Equal
        }
    });

    match result {
        // SAFETY: `idx` is offset within base limits derived from `binary_search_by()` argument.
        // And idx*size is `nmemb * size` as per assignment in `binary_search_by()`
        Ok(idx) => unsafe { base.add(idx * size) as *mut c_void },
        Err(_) => core::ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    /// # Safety
    ///
    /// Input `a` and `b` must be valid pointers to `i32`.
    unsafe extern "C" fn cmp_i32(a: *const c_void, b: *const c_void) -> c_int {
        // SAFETY: `a` is valid pointer to i32.
        let a_val = unsafe { *(a as *const i32) };
        // SAFETY: `b` is valid pointer to i32.
        let b_val = unsafe { *(b as *const i32) };
        if a_val < b_val {
            -1
        } else if a_val > b_val {
            1
        } else {
            0
        }
    }

    fn test_bsearch(arr: &[i32], key: i32) -> Option<usize> {
        let base = arr.as_ptr();
        // SAFETY: Input array reference is verified valid for memory range and length.
        let res = unsafe {
            bsearch_rust(
                &key as *const i32 as *const c_void,
                base as *const c_void,
                arr.len(),
                size_of::<i32>(),
                cmp_i32,
            )
        };
        if res.is_null() {
            None
        } else {
            // SAFETY: res is non-NULL and belongs to memory range originating at base.
            let offset = unsafe { (res as *const u8).offset_from(base as *const u8) };
            Some(offset as usize / size_of::<i32>())
        }
    }

    #[test]
    fn test_bsearch_found() {
        let arr = [1, 2, 3, 4, 5];

        assert_eq!(test_bsearch(&arr, 1), Some(0));
        assert_eq!(test_bsearch(&arr, 3), Some(2));
        assert_eq!(test_bsearch(&arr, 5), Some(4));
    }

    #[test]
    fn test_bsearch_not_found() {
        let arr = [1, 2, 4, 5];
        assert_eq!(test_bsearch(&arr, 3), None);
    }

    #[test]
    fn test_bsearch_empty_array() {
        let arr = [];
        assert_eq!(test_bsearch(&arr, 1), None);
    }

    #[test]
    fn test_bsearch_duplicate_elements() {
        let arr = [1, 2, 2, 2, 3];
        let res = test_bsearch(&arr, 2);
        assert!(res.is_some());
        let idx = res.unwrap();
        assert!(idx >= 1 && idx <= 3);
    }
}
