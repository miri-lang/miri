// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_operation_outputs, assert_runs_with_output};

#[test]
fn test_if_else_inline() {
    assert_operation_outputs(&[("if true: 1 else: 0", "1"), ("if false: 1 else: 0", "0")]);
}

#[test]
fn test_if_else_block() {
    assert_runs_with_output(
        r#"
use system.io

let x = 10
let y = if x > 5
    x * 2
else
    x
print(f"{y}")
        "#,
        "20",
    );
}

#[test]
fn test_if_else_if_else() {
    assert_runs_with_output(
        r#"
use system.io
let x = 5
let y = if x > 10
    100
else if x > 3
    50
else
    0
print(f"{y}")
    "#,
        "50",
    );
}

#[test]
fn test_unless_inline() {
    assert_operation_outputs(&[
        ("unless false: 1 else: 0", "1"),
        ("unless true: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_nested_if() {
    assert_runs_with_output(
        r#"
use system.io
let x = 15
let y = if x > 10
    if x > 20
        3
    else
        2
else
    1
print(f"{y}")
        "#,
        "2",
    );
}

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
        "25",
    ); // 1+3+5+7+9 = 25
}

#[test]
fn test_comparison_operators() {
    assert_operation_outputs(&[
        ("if 5 > 3: 1 else: 0", "1"),
        ("if 5 < 3: 1 else: 0", "0"),
        ("if 5 >= 5: 1 else: 0", "1"),
        ("if 5 <= 5: 1 else: 0", "1"),
        ("if 5 == 5: 1 else: 0", "1"),
        ("if 5 != 5: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_logical_and() {
    assert_operation_outputs(&[
        ("if true and true: 1 else: 0", "1"),
        ("if true and false: 1 else: 0", "0"),
        ("if false and true: 1 else: 0", "0"),
        ("if false and false: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_logical_or() {
    assert_operation_outputs(&[
        ("if true or true: 1 else: 0", "1"),
        ("if true or false: 1 else: 0", "1"),
        ("if false or true: 1 else: 0", "1"),
        ("if false or false: 1 else: 0", "0"),
    ]);
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

// =============================================================================
// Unless block statement
// =============================================================================

#[test]
fn test_unless_block() {
    assert_runs_with_output(
        r#"
use system.io
var x = 3
unless x > 10
    x = x + 1
print(f"{x}")
    "#,
        "4",
    );
}

#[test]
fn test_unless_block_condition_true() {
    assert_runs_with_output(
        r#"
use system.io
var x = 20
unless x > 10
    x = 0
print(f"{x}")
    "#,
        "20", // condition is true → body skipped
    );
}

#[test]
fn test_unless_block_with_else() {
    assert_runs_with_output(
        r#"
use system.io
var x = 15
unless x < 10
    x = 99
else
    x = 0
print(f"{x}")
    "#,
        "99", // condition is false → unless body runs
    );
}

// =============================================================================
// Until loop (pre-test, stops when condition becomes true)
// =============================================================================

#[test]
fn test_until_loop() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
var i = 0
until i >= 5
    x = x + i
    i = i + 1
print(f"{x}")
    "#,
        "10", // 0+1+2+3+4 = 10
    );
}

#[test]
fn test_until_loop_never_enters() {
    assert_runs_with_output(
        r#"
use system.io
var x = 42
until true
    x = 0
print(f"{x}")
    "#,
        "42", // condition true from start → body never runs
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

// =============================================================================
// Do-while loop (post-test: body executes at least once)
// =============================================================================

#[test]
fn test_do_while_basic() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
do
    x = x + 1
while x < 5
print(f"{x}")
    "#,
        "5",
    );
}

#[test]
fn test_do_while_executes_once() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
do
    x = x + 1
while false
print(f"{x}")
    "#,
        "1", // body runs once even though condition is immediately false
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

// =============================================================================
// Do-until loop (post-test inverted: stops when condition becomes true)
// =============================================================================

#[test]
fn test_do_until_basic() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
do
    x = x + 1
until x >= 5
print(f"{x}")
    "#,
        "5",
    );
}

#[test]
fn test_do_until_executes_once() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
do
    x = x + 1
until true
print(f"{x}")
    "#,
        "1", // body runs once before the condition is checked
    );
}

// =============================================================================
// Forever loop (infinite loop, requires break to exit)
// =============================================================================

#[test]
fn test_forever_with_break() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
forever
    x = x + 1
    if x >= 5
        break
print(f"{x}")
    "#,
        "5",
    );
}

#[test]
fn test_forever_with_continue() {
    assert_runs_with_output(
        r#"
use system.io
var sum = 0
var i = 0
forever
    i = i + 1
    if i > 9
        break
    if i % 2 == 0
        continue
    sum = sum + i
print(f"{sum}")
    "#,
        "25", // 1+3+5+7+9 = 25
    );
}

// =============================================================================
// For-range inclusive (1..=n includes n)
// =============================================================================

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

// =============================================================================
// Break and continue in various loop types
// =============================================================================

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

// =============================================================================
// Break only exits innermost loop (nested loops)
// =============================================================================

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

// =============================================================================
// Edge cases
// =============================================================================

#[test]
fn test_while_zero_iterations() {
    assert_runs_with_output(
        r#"
use system.io
var x = 99
while false
    x = 0
print(f"{x}")
    "#,
        "99",
    );
}

#[test]
fn test_for_empty_range() {
    assert_runs_with_output(
        r#"
use system.io
var count = 0
for i in 5..5
    count = count + 1
print(f"{count}")
    "#,
        "0", // Span::new(5, 5) is empty (exclusive)
    );
}

#[test]
fn test_deeply_nested_if_else() {
    assert_runs_with_output(
        r#"
use system.io
let x = 7
let r = if x < 0
    -1
else if x == 0
    0
else if x < 5
    1
else if x < 10
    2
else
    3
print(f"{r}")
    "#,
        "2",
    );
}
