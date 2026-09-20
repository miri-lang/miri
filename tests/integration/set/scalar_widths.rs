// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Set operations whose element type is a scalar narrower or wider than `int`,
//! which compiles a per-element-width copy of the set's own methods.

use crate::integration::utils::*;

#[test]
fn test_set_i32_remove_returns_and_shrinks() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<i32>({})
    s.add(200)
    s.add(201)
    let removed = s.remove(200)
    println(f"{removed}")
    println(f"{s.length()}")
    println(f"{s.contains(201)}")
"#,
        "true
1
true",
    );
}

#[test]
fn test_set_f64_remove_returns_and_shrinks() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<f64>({})
    s.add(1.5)
    s.add(2.5)
    let removed = s.remove(1.5)
    println(f"{removed}")
    println(f"{s.length()}")
    println(f"{s.contains(2.5)}")
"#,
        "true
1
true",
    );
}

#[test]
fn test_set_int_remove_still_works() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set({1, 2})
    let removed = s.remove(1)
    println(f"{removed}")
    println(f"{s.length()}")
"#,
        "true
1",
    );
}

#[test]
#[ignore = "a set element wider than a value word is truncated to its low eight bytes at the runtime call: the entry point takes the element as a pointer-sized integer and copies elem_size bytes from the address of that stack parameter, so the upper half never reaches the set and two elements sharing a low word compare equal. Store and lookup are wrong together, so no partial fix helps"]
fn test_set_i128_distinguishes_elements_by_the_whole_value() {
    // `i128::MAX` and `-1` fill their low eight bytes with the same ones and
    // differ only above bit 63, so a set that compared a value word alone would
    // call them equal and hold one of them for both.
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let twin i128 = -1
    var s = Set<i128>()
    s.add(big)
    println(f"{s.contains(big)} {s.contains(twin)}")
    s.add(twin)
    println(f"{s.length()} {s.contains(twin)}")
"#,
        "true false
2 true",
    );
}

#[test]
#[ignore = "a set element wider than a value word is truncated to its low eight bytes at the runtime call: the entry point takes the element as a pointer-sized integer and copies elem_size bytes from the address of that stack parameter, so the upper half never reaches the set and two elements sharing a low word compare equal. Store and lookup are wrong together, so no partial fix helps"]
fn test_set_i128_removes_only_the_element_asked_for() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let twin i128 = -1
    var s = Set<i128>()
    s.add(big)
    s.add(twin)
    println(f"{s.remove(twin)} {s.length()}")
    println(f"{s.contains(big)} {s.contains(twin)}")
"#,
        "true 1
true false",
    );
}

#[test]
#[ignore = "a set element wider than a value word is truncated to its low eight bytes at the runtime call: the entry point takes the element as a pointer-sized integer and copies elem_size bytes from the address of that stack parameter, so the upper half never reaches the set and two elements sharing a low word compare equal. Store and lookup are wrong together, so no partial fix helps"]
fn test_set_u128_distinguishes_elements_above_the_low_word() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let all_ones u128 = ~(0 as u128)
    let low_ones u128 = 18446744073709551615
    var s = Set<u128>()
    s.add(all_ones)
    println(f"{s.contains(all_ones)} {s.contains(low_ones)}")
    s.add(low_ones)
    println(f"{s.length()}")
"#,
        "true false
2",
    );
}

#[test]
fn test_set_i128_deduplicates_the_same_element() {
    // Passes today, because the same element has the same low word either
    // way. It guards against a widening fix over-correcting into storing a
    // duplicate, and proves nothing about the upper half on its own.
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let big i128 = 170141183460469231731687303715884105727
    var s = Set<i128>()
    s.add(big)
    s.add(big)
    println(f"{s.length()}")
"#,
        "1",
    );
}
