// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_while_loop() {
    assert_runs_with_output(
        r#"
use system.io
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
use system.io
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
use system.io
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
use system.io
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
use system.io
var count = 0
for i in 3..=3
    count = count + 1
print(f"{count}")
    "#,
        "1", // single element
    );
}
