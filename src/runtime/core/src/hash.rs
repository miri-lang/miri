// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Hashing: FNV-1a over raw byte sequences, which `MiriSet` and `MiriMap`
//! place their elements by, and the helpers a program's `hash` methods are
//! built from.

/// FNV-1a hash for raw byte sequences.
pub(crate) fn fnv1a(data: *const u8, len: usize) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for i in 0..len {
        hash ^= unsafe { *data.add(i) } as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
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
            return super::fnv1a(std::ptr::null(), 0) as i64;
        }
        super::fnv1a((*string).data, (*string).len) as i64
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
