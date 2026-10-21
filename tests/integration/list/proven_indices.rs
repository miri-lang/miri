// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Loops whose index a guard may or may not prove in bounds. Every program
//! here reads or writes past an end somewhere, so each must still stop with
//! the out-of-bounds report: an access is emitted unchecked only where it
//! cannot fail. Each also runs under the heap guard.

use super::utils::*;
use crate::utils::miri_run_with_env;

fn expect_out_of_bounds(body: &str, report: &str) {
    let source = format!("use system.io\nuse system.collections.list\n\n{body}");
    assert_runtime_error(&source, report);
    let guarded = miri_run_with_env(&source, "MIRI_HEAP_GUARD", "1");
    assert!(
        !guarded.success && guarded.stderr.contains(report),
        "under the heap guard:\n{}",
        guarded.output()
    );
}

#[test]
fn test_an_index_starting_below_zero_still_stops_the_program() {
    expect_out_of_bounds(
        r#"
fn main()
    let l = List([1, 2, 3])
    var i = 0 - 1
    var t = 0
    while i < l.length()
        t = t + l[i]
        i += 1
    println(f"{t}")
"#,
        "the index is -1",
    );
}

#[test]
fn test_an_index_counting_down_past_zero_still_stops_the_program() {
    expect_out_of_bounds(
        r#"
fn main()
    let l = List([1, 2, 3])
    var i = 2
    var t = 0
    while i < l.length()
        t = t + l[i]
        i -= 1
    println(f"{t}")
"#,
        "the index is -1",
    );
}

#[test]
fn test_a_less_or_equal_guard_still_stops_the_program_one_past_the_end() {
    expect_out_of_bounds(
        r#"
fn main()
    let l = List([1, 2, 3])
    var i = 0
    var t = 0
    while i <= l.length()
        t = t + l[i]
        i += 1
    println(f"{t}")
"#,
        "the len is 3 but the index is 3",
    );
}

#[test]
fn test_an_index_bumped_between_the_guard_and_the_access_still_stops_the_program() {
    expect_out_of_bounds(
        r#"
fn main()
    let l = List([1, 2, 3])
    var i = 0
    var t = 0
    while i < l.length()
        i += 1
        t = t + l[i]
    println(f"{t}")
"#,
        "the len is 3 but the index is 3",
    );
}

#[test]
fn test_a_list_replaced_after_its_length_was_taken_still_stops_the_program() {
    expect_out_of_bounds(
        r#"
fn main()
    var l = List([1, 2, 3])
    let n = l.length()
    var i = 0
    var t = 0
    while i < n
        if i == 1
            l = List([7])
        t = t + l[i]
        i += 1
    println(f"{t}")
"#,
        "the len is 1 but the index is 1",
    );
}

#[test]
fn test_an_index_from_a_parameter_still_stops_the_program() {
    expect_out_of_bounds(
        r#"
fn total(l [int], start int) int
    var i = start
    var t = 0
    while i < l.length()
        t = t + l[i]
        i += 1
    t

fn main()
    println(f"{total(List([1, 2, 3]), 0 - 2)}")
"#,
        "the index is -2",
    );
}

#[test]
fn test_a_push_between_the_length_and_the_access_reads_the_grown_list() {
    let source = r#"
use system.io
use system.collections.list

fn main()
    var l = List([1, 2, 3])
    let n = l.length()
    var i = 0
    var t = 0
    while i < n
        l.push(i * 10)
        t = t + l[i + n]
        i += 1
    println(f"{t} {l.length()}")
"#;
    assert_runs_with_output(source, "30 6\n");
    assert_heap_guard_output(source, "30 6\n");
}
