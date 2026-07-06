// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! FNV-1a hash algorithm for raw byte sequences.
//!
//! Used by `MiriSet` and `MiriMap` for consistent hashing across collection types.

/// FNV-1a hash for raw byte sequences.
pub(crate) fn fnv1a(data: *const u8, len: usize) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for i in 0..len {
        hash ^= unsafe { *data.add(i) } as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
