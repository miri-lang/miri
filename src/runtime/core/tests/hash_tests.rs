// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_core::hash::ffi::*;

#[test]
fn test_hash_combine_depends_on_every_part_and_their_order() {
    let ab = miri_rt_hash_combine(miri_rt_hash_combine(0, 1), 2);
    let ba = miri_rt_hash_combine(miri_rt_hash_combine(0, 2), 1);
    let aa = miri_rt_hash_combine(miri_rt_hash_combine(0, 1), 1);
    assert_ne!(ab, ba);
    assert_ne!(ab, aa);
    assert_eq!(ab, miri_rt_hash_combine(miri_rt_hash_combine(0, 1), 2));
}

#[test]
fn test_float_hash_agrees_with_float_equality() {
    assert_eq!(miri_rt_float_hash(0.0), miri_rt_float_hash(-0.0));
    assert_eq!(miri_rt_float_hash(f64::NAN), miri_rt_float_hash(-f64::NAN));
    assert_ne!(miri_rt_float_hash(1.0), miri_rt_float_hash(2.0));
}

#[test]
fn test_string_hash_reads_content_and_a_null_string_is_empty() {
    unsafe {
        let text = |t: &str| miri_runtime_core::miri_rt_string_from_raw(t.as_ptr(), t.len());
        let (pear, other_pear, plum, empty) = (text("pear"), text("pear"), text("plum"), text(""));
        assert_eq!(miri_rt_string_hash(pear), miri_rt_string_hash(other_pear));
        assert_ne!(miri_rt_string_hash(pear), miri_rt_string_hash(plum));
        assert_eq!(
            miri_rt_string_hash(std::ptr::null()),
            miri_rt_string_hash(empty)
        );
        for string in [pear, other_pear, plum, empty] {
            miri_runtime_core::miri_rt_string_free(string);
        }
    }
}

/// The hash of `bytes` as a string's content.
fn content_hash(bytes: &[u8]) -> i64 {
    unsafe {
        let string = miri_runtime_core::miri_rt_string_from_raw(bytes.as_ptr(), bytes.len());
        let hash = miri_rt_string_hash(string);
        miri_runtime_core::miri_rt_string_free(string);
        hash
    }
}

#[test]
fn test_string_hash_reads_every_byte_at_every_length() {
    for len in 1..=40usize {
        let base: Vec<u8> = (0..len).map(|i| b'a' + (i % 26) as u8).collect();
        let hash = content_hash(&base);
        for at in 0..len {
            let mut changed = base.clone();
            changed[at] ^= 0x20;
            assert_ne!(hash, content_hash(&changed), "length {len}, byte {at}");
        }
    }
}

#[test]
fn test_string_hash_tells_a_prefix_from_its_zero_padding() {
    for len in 0..=24usize {
        let short = vec![b'x'; len];
        let mut padded = short.clone();
        padded.push(0);
        assert_ne!(content_hash(&short), content_hash(&padded), "length {len}");
    }
}

#[test]
fn test_string_hash_spreads_short_keys_over_low_and_high_bits() {
    let hashes: Vec<u64> = (0..4096)
        .map(|i| content_hash(format!("w{i}").as_bytes()) as u64)
        .collect();
    let low: std::collections::HashSet<u64> = hashes.iter().map(|h| h & 1023).collect();
    let high: std::collections::HashSet<u64> = hashes.iter().map(|h| h >> 57).collect();
    assert!(low.len() > 900, "{} of 1024 low buckets used", low.len());
    assert_eq!(high.len(), 128);
}
