// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_while_loop() {
    assert_runs_with_output(
        r#"
var x = 0
var i = 0
while i < 5
    x = x + i
    i = i + 1
print(f"{x}")
    "#,
        "10",
    ); // 0+1+2+3+4 = 10
}

#[test]
fn test_for_loop_range() {
    assert_runs_with_output(
        r#"
var sum = 0
for i in 1..5
    sum = sum + i
print(f"{sum}")
    "#,
        "10",
    ); // 1+2+3+4 = 10
}

#[test]
fn test_nested_loops() {
    assert_runs_with_output(
        r#"
var sum = 0
for i in 1..4
    for j in 1..4
        sum = sum + 1
print(f"{sum}")
    "#,
        "9",
    ); // 3 * 3 = 9
}

#[test]
fn test_for_range_inclusive() {
    assert_runs_with_output(
        r#"
var sum = 0
for i in 1..=5
    sum = sum + i
print(f"{sum}")
    "#,
        "15", // 1+2+3+4+5 = 15
    );
}

#[test]
fn test_for_range_inclusive_single() {
    assert_runs_with_output(
        r#"
var count = 0
for i in 3..=3
    count = count + 1
print(f"{count}")
    "#,
        "1", // single element
    );
}

// ── Invalid number of loop variables ────────────────────────────────────

#[test]
fn test_for_loop_three_variables_over_iterable_is_error() {
    assert_compiler_error(
        r#"
use system.collections.list

let items = List([1, 2, 3])
for i, j, k in items
    println("x")
    "#,
        "Invalid number of loop variables",
    );
}

/// An inclusive range that ends at its type's maximum runs once per value and
/// ends: the loop stops at the last value instead of computing one past it.
#[test]
fn test_inclusive_range_ending_at_the_maximum_runs_each_value_and_ends() {
    assert_runs_with_output(
        r#"
fn main()
    var n = 0
    var last = 0
    for i in 9223372036854775805..=9223372036854775807
        n = n + 1
        last = i
    println(f"{n} {last == 9223372036854775807}")
"#,
        "3 true",
    );
}

/// The exclusive form up to the maximum stops one short, as it always did.
#[test]
fn test_exclusive_range_ending_at_the_maximum_stops_before_it() {
    assert_runs_with_output(
        r#"
fn main()
    var n = 0
    for i in 9223372036854775805..9223372036854775807
        n = n + 1
    println(f"{n}")
"#,
        "2",
    );
}

/// A range's end is evaluated once, before the first iteration: a call
/// written there runs once, and writing a binding the end reads changes no
/// later test of the loop.
#[test]
fn a_for_range_end_is_evaluated_once() {
    assert_runs_with_output(
        r#"
fn lim() int
    println("lim")
    return 3

fn main()
    for i in 0..lim()
        println(f"{i}")
    var n = 3
    for i in 0..n
        n = 1
        println(f"n{i}")
    var m = 1
    for i in 0..=m
        m = 5
        println(f"m{i}")
    println(f"{n} {m}")
"#,
        "lim\n0\n1\n2\nn0\nn1\nn2\nm0\nm1\n1 5",
    );
}
