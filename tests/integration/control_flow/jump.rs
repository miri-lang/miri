// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_break_in_while() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
while true
    x = x + 1
    if x >= 5
        break
print(f"{x}")
    "#,
        "5",
    );
}

#[test]
fn test_continue_in_for() {
    assert_runs_with_output(
        r#"
use system.io
var sum = 0
for i in 1..10
    if i % 2 == 0
        continue
    sum = sum + i
print(f"{sum}")
    "#,
        "25", // 1+3+5+7+9 = 25
    );
}

#[test]
fn test_until_loop_with_break() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
until false
    x = x + 1
    if x >= 3
        break
print(f"{x}")
    "#,
        "3",
    );
}

#[test]
fn test_do_while_with_break() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
do
    x = x + 1
    if x >= 3
        break
while true
print(f"{x}")
    "#,
        "3",
    );
}

#[test]
fn test_do_while_with_continue() {
    assert_runs_with_output(
        r#"
use system.io
var sum = 0
var i = 0
do
    i = i + 1
    if i % 2 == 0
        continue
    sum = sum + i
while i < 9
print(f"{sum}")
    "#,
        "25", // 1+3+5+7+9 = 25
    );
}

#[test]
fn test_break_in_for() {
    assert_runs_with_output(
        r#"
use system.io
var found = 0
for i in 1..10
    if i == 5
        found = i
        break
print(f"{found}")
    "#,
        "5",
    );
}

#[test]
fn test_continue_in_while() {
    assert_runs_with_output(
        r#"
use system.io
var sum = 0
var i = 0
while i < 10
    i = i + 1
    if i % 2 == 0
        continue
    sum = sum + i
print(f"{sum}")
    "#,
        "25", // 1+3+5+7+9 = 25
    );
}

#[test]
fn test_break_in_for_skips_rest() {
    assert_runs_with_output(
        r#"
use system.io
var sum = 0
for i in 1..10
    if i > 4
        break
    sum = sum + i
print(f"{sum}")
    "#,
        "10", // 1+2+3+4 = 10
    );
}

#[test]
fn test_break_exits_inner_loop_only() {
    assert_runs_with_output(
        r#"
use system.io
var count = 0
for i in 1..4
    for j in 1..10
        if j > 2
            break
        count = count + 1
print(f"{count}")
    "#,
        "6", // outer: 3 iters; inner: 2 each = 6
    );
}

#[test]
fn test_continue_inner_loop_only() {
    assert_runs_with_output(
        r#"
use system.io
var count = 0
for i in 1..4
    for j in 1..5
        if j == 2
            continue
        count = count + 1
print(f"{count}")
    "#,
        "9", // outer: 3 iters; inner: 3 each (skips j=2) = 9
    );
}
