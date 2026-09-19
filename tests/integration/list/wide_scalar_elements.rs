// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for `List<T>` whose element is a scalar wider than a value word
//! (`i128`, `u128`).
//!
//! Such an element cannot travel in the single pointer-sized parameter the
//! word-passing entry points receive, so it is handed to the list by address.
//! Each round-trip is checked against a value that shares its low word with a
//! different one, which only holds when the upper half survived the store.

use crate::integration::utils::*;

#[test]
fn test_list_push_i128_stores_the_element() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<i128>()
    l.push(5)
    println(f"{l[0]}")
"#,
        "5",
    );
}

#[test]
fn test_list_push_u128_stores_the_element() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<u128>()
    l.push(7)
    println(f"{l[0]}")
"#,
        "7",
    );
}

#[test]
fn test_list_push_i128_keeps_the_upper_half() {
    // `i128::MAX` and `-1` share every bit of their low word and differ only
    // above it, so equality here reads the half a value word would have lost.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let low_word_twin i128 = -1
    var l = List<i128>()
    l.push(big)
    println(f"{l[0] == big}")
    println(f"{l[0] == low_word_twin}")
"#,
        "true
false",
    );
}

#[test]
fn test_list_push_u128_keeps_the_upper_half() {
    // Two to the sixty-fourth has a zero low word, so a list that stored only
    // that word would hand back zero.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let big u128 = 18446744073709551616
    var l = List<u128>()
    l.push(big)
    println(f"{l[0] == big}")
    println(f"{l[0] == 0}")
"#,
        "true
false",
    );
}

#[test]
fn test_list_push_i128_appends_in_order() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<i128>()
    l.push(1)
    l.push(2)
    l.push(3)
    println(f"{l.length()}")
    println(f"{l[0]} {l[1]} {l[2]}")
"#,
        "3
1 2 3",
    );
}

#[test]
fn test_list_insert_i128_shifts_later_elements() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<i128>()
    l.push(10)
    l.push(20)
    l.insert(0, 5)
    l.insert(3, 30)
    println(f"{l[0]} {l[1]} {l[2]} {l[3]}")
"#,
        "5 10 20 30",
    );
}

#[test]
fn test_list_insert_i128_keeps_the_upper_half() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let low_word_twin i128 = -1
    var l = List<i128>()
    l.push(1)
    l.insert(0, big)
    println(f"{l[0] == big}")
    println(f"{l[0] == low_word_twin}")
"#,
        "true
false",
    );
}

#[test]
fn test_list_set_i128_keeps_the_upper_half() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let low_word_twin i128 = -1
    var l = List<i128>()
    l.push(1)
    l.push(2)
    l.set(1, big)
    println(f"{l[1] == big}")
    println(f"{l[1] == low_word_twin}")
"#,
        "true
false",
    );
}

#[test]
fn test_list_i128_index_assignment_keeps_the_upper_half() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let low_word_twin i128 = -1
    var l = List<i128>()
    l.push(1)
    l.push(2)
    l[1] = big
    println(f"{l[1] == big}")
    println(f"{l[1] == low_word_twin}")
"#,
        "true
false",
    );
}

#[test]
fn test_list_built_from_a_literal_keeps_the_upper_half() {
    // Printing an element only shows its low word, so the comparison is what
    // proves the whole slot was written.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let one i128 = 1
    var l = List<i128>([big, one])
    println(f"{l[0] == big} {l[1] == one}")
    println(f"{l[0] == one} {l[1] == big}")
"#,
        "true true
false false",
    );
}

#[test]
fn test_list_built_from_a_literal_infers_the_wide_element() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let big i128 = 170141183460469231731687303715884105727
    var l = List([big])
    l.push(big)
    println(f"{l[0] == big} {l[1] == big}")
"#,
        "true true",
    );
}

#[test]
#[ignore = "an integer literal is typed as the default int whatever it is written into, so the elements of List<i128>([1, 2, 3]) reach the list's wider slots without being extended and only their low word is written; a literal already typed i128 round-trips (see the sibling tests)"]
fn test_list_built_from_int_literals_widens_them_to_the_slot() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let one i128 = 1
    var l = List<i128>([3, 1, 2])
    println(f"{l[1] == one}")
"#,
        "true",
    );
}

#[test]
fn test_list_i128_pop_returns_the_last_element() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let big i128 = 170141183460469231731687303715884105727
    var l = List<i128>()
    l.push(1)
    l.push(big)
    let last = l.pop()
    match last
        Some(v): println(f"{v == big}")
        None: println("none")
    println(f"{l.length()}")
"#,
        "true
1",
    );
}

#[test]
fn test_list_i128_remove_at_shifts_later_elements() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<i128>()
    l.push(1)
    l.push(2)
    l.push(3)
    let taken = l.remove_at(1)
    match taken
        Some(v): println(f"{v}")
        None: println("none")
    println(f"{l[0]} {l[1]}")
"#,
        "2
1 3",
    );
}

#[test]
fn test_list_insert_i128_past_the_end_leaves_the_list_unchanged() {
    // The by-address entry point reports the same refusal the word-passing one
    // does, so an index past the end stores nothing and grows nothing.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<i128>()
    l.push(1)
    l.insert(9, 2)
    println(f"{l.length()}")
    println(f"{l[0]}")
"#,
        "1
1",
    );
}

#[test]
fn test_list_i128_index_past_the_end_reports_the_bound() {
    assert_runtime_error(
        r#"
use system.collections.list

fn main()
    var l = List<i128>()
    l.push(1)
    println(f"{l[5]}")
"#,
        "the len is 1 but the index is 5",
    );
}

#[test]
fn test_list_push_negative_i128_sign_extends() {
    // A negative `int` literal reaches a wider slot through the same cast, and
    // zero-extending it there would read back as a large positive number.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let expected i128 = -7
    var l = List<i128>()
    l.push(-7)
    println(f"{l[0] == expected}")
    println(f"{l[0] < 0}")
"#,
        "true
true",
    );
}
