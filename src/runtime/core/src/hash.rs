// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Hashing: the byte hash `MiriSet` and `MiriMap` place their elements by,
//! and the helpers a program's `hash` methods are built from.

/// Odd multipliers whose products spread every input bit over the whole word.
const SPREAD: u64 = 0x9e37_79b9_7f4a_7c15;
const STIR: u64 = 0xd6e8_feb8_6659_fd93;

/// The low and high halves of the 128-bit product of `a` and `b`, folded into
/// one word, so a change in any bit of either reaches the top and bottom bits.
#[inline]
fn fold(a: u64, b: u64) -> u64 {
    let product = u128::from(a) * u128::from(b);
    (product as u64) ^ ((product >> 64) as u64)
}

/// The hash of the `len` bytes at `data`, read eight at a time. The length is
/// mixed in first, so a sequence and the same sequence padded with zero bytes
/// differ, and the result is folded once more so both its low bits (the slot)
/// and its high bits (the slot tag) depend on every byte.
///
/// `data` must point at `len` readable bytes; it may be null when `len` is 0.
#[inline]
pub(crate) fn hash_bytes(data: *const u8, len: usize) -> u64 {
    let mut hash = (len as u64).wrapping_mul(SPREAD) ^ STIR;
    // SAFETY: every read below lies inside the `len` bytes at `data`.
    unsafe {
        let word = |at: usize| (data.add(at) as *const u64).read_unaligned();
        let half = |at: usize| u64::from((data.add(at) as *const u32).read_unaligned());
        let tail = match len {
            0 => 0,
            1..=3 => {
                let byte = |at: usize| u64::from(*data.add(at));
                byte(0) | byte(len / 2) << 8 | byte(len - 1) << 16
            }
            4..=8 => half(0) | half(len - 4) << 32,
            _ => {
                let mut at = 0;
                while len - at > 8 {
                    hash = fold(hash ^ word(at), SPREAD);
                    at += 8;
                }
                word(len - 8)
            }
        };
        fold(fold(hash ^ tail, SPREAD), STIR)
    }
}

/// The helpers compiled code hashes values through: the hash a type derives
/// from its fields, and the one a program writes in its own `hash` method.
pub mod ffi {
    use crate::guard;
    use crate::string::MiriString;

    /// `seed` with `value` mixed into it, so a hash over several parts depends
    /// on every part and on their order.
    #[no_mangle]
    pub extern "C" fn miri_rt_hash_combine(seed: i64, value: i64) -> i64 {
        let mixed = (value as u64)
            .wrapping_mul(0x9e37_79b9_7f4a_7c15)
            .rotate_left(31);
        ((seed as u64 ^ mixed).wrapping_mul(0x0000_0100_0000_01b3)) as i64
    }

    /// The hash of a string's content; a null string hashes as the empty one,
    /// which is what `==` makes it.
    ///
    /// # Safety
    /// - `string` must be a valid `MiriString` pointer, or null.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_string_hash(string: *const MiriString) -> i64 {
        guard::guard_check(string as *mut u8);
        if string.is_null() || (*string).data.is_null() {
            return super::hash_bytes(std::ptr::null(), 0) as i64;
        }
        super::hash_bytes((*string).data, (*string).len) as i64
    }

    /// The hash of a float's number: `-0.0` and `0.0` hash alike, as does every
    /// NaN, matching the rule a set or map matches floats by. A narrower float
    /// is handed over widened, which keeps its number.
    #[no_mangle]
    pub extern "C" fn miri_rt_float_hash(value: f64) -> i64 {
        let bits = value.to_bits().to_ne_bytes();
        // SAFETY: `bits` holds the eight bytes of one double.
        unsafe { crate::element_identity::float_value_bits(bits.as_ptr(), bits.len()) as i64 }
    }
}
