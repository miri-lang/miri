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
