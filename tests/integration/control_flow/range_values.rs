// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A range is a value: one bound to a name, passed to a function or returned
//! from one is iterated the same as a range written in the loop header.

use super::utils::*;

#[test]
fn test_a_range_bound_to_a_name_is_iterated() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let r = 2..5
    var total = 0
    for i in r
        total = total * 10 + i
    println(f"{total}")
"#,
        "234",
    );
}

#[test]
fn test_an_inclusive_range_returned_from_a_function_reaches_its_end() {
    assert_runs_with_output(
        r#"
use system.io

fn span(a int, b int) Range<int>
    a..=b

fn main()
    for j in span(7, 9)
        println(f"{j}")
"#,
        "7\n8\n9",
    );
}

#[test]
fn test_a_range_passed_as_a_parameter_is_iterated_and_can_be_empty() {
    assert_runs_with_output(
        r#"
use system.io

fn sum(r Range<int>) int
    var total = 0
    for i in r
        total = total + i
    total

fn main()
    println(f"{sum(1..4)} {sum(5..5)} {sum(5..=5)} {sum(3..1)}")
"#,
        "6 0 5 0",
    );
}

#[test]
fn test_breaking_out_of_a_range_value_loop_leaves_at_once() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let r = 0..100
    var last = -1
    for i in r
        if i == 3
            break
        last = i
    println(f"{last}")
"#,
        "2",
    );
}
