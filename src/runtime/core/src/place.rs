// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Putting a new value in a caller's place and handing back the one it held.
//!
//! The place, the new value and the storage for the old one each arrive as an
//! element does: the address of the bytes and how many bytes the value is.
//! Only bytes move. The reference the place held leaves with the bytes written
//! to the caller's storage, and the one the new value carried stays with the
//! bytes written into the place, so no count changes: the compiler has already
//! funded the place's new reference and hands the old one to the caller.

use std::ptr;

/// Write the `place_bytes` bytes at `place` into `old`, then the bytes at
/// `new` into `place`.
///
/// The three widths are one value's, so they agree; a call where they do not
/// is a compiler defect, and the process stops with a report rather than
/// write a part of a value or past one.
///
/// # Safety
/// - `place` must be readable and writable for `place_bytes` bytes.
/// - `new` must be readable for `new_bytes` bytes.
/// - `old` must be writable for `old_bytes` bytes.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_replace(
    place: *mut u8,
    place_bytes: usize,
    new: *const u8,
    new_bytes: usize,
    old: *mut u8,
    old_bytes: usize,
) {
    if place_bytes != new_bytes || place_bytes != old_bytes {
        eprintln!(
            "Runtime error: a value of {place_bytes} bytes was replaced by one of {new_bytes} bytes into storage of {old_bytes} bytes"
        );
        std::process::abort();
    }
    ptr::copy(place, old, place_bytes);
    ptr::copy(new, place, place_bytes);
}

#[cfg(test)]
mod tests {
    use super::miri_rt_replace;

    #[test]
    fn the_old_bytes_reach_the_storage_and_the_new_ones_the_place() {
        let mut place = 7u64.to_ne_bytes();
        let new = 9u64.to_ne_bytes();
        let mut old = [0u8; 8];
        unsafe {
            miri_rt_replace(
                place.as_mut_ptr(),
                place.len(),
                new.as_ptr(),
                new.len(),
                old.as_mut_ptr(),
                old.len(),
            );
        }
        assert_eq!(u64::from_ne_bytes(old), 7);
        assert_eq!(u64::from_ne_bytes(place), 9);
    }
}
