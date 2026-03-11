// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_and_with_comparisons() {
    assert_operation_outputs(&[
        ("if 5 > 3 and 10 < 20: 1 else: 0", "1"),
        ("if 5 > 3 and 10 > 20: 1 else: 0", "0"),
        ("if 5 < 3 and 10 < 20: 1 else: 0", "0"),
        ("if 5 < 3 and 10 > 20: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_or_with_comparisons() {
    assert_operation_outputs(&[
        ("if 5 > 3 or 10 > 20: 1 else: 0", "1"),
        ("if 5 < 3 or 10 < 20: 1 else: 0", "1"),
        ("if 5 < 3 or 10 > 20: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_not_comparison_result() {
    assert_operation_outputs(&[
        ("if not (5 == 3): 1 else: 0", "1"),
        ("if not (5 == 5): 1 else: 0", "0"),
        ("if not (5 > 3): 1 else: 0", "0"),
        ("if not (3 > 5): 1 else: 0", "1"),
    ]);
}

#[test]
fn test_double_not() {
    assert_operation_outputs(&[
        ("if not not true: 1 else: 0", "1"),
        ("if not not false: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_not_with_and_or() {
    assert_operation_outputs(&[
        ("if not (true and false): 1 else: 0", "1"),
        ("if not (true and true): 1 else: 0", "0"),
        ("if not (false or false): 1 else: 0", "1"),
        ("if not (false or true): 1 else: 0", "0"),
    ]);
}

#[test]
fn test_and_short_circuits_on_false_lhs() {
    assert_runs_with_output(
        r#"
use system.io

var x = 0
let result = if false and (1 / x == 0)
    1
else
    0
println(f"{result}")
"#,
        "0",
    );
}

#[test]
fn test_or_short_circuits_on_true_lhs() {
    assert_runs_with_output(
        r#"
use system.io

var x = 0
let result = if true or (1 / x == 0)
    1
else
    0
println(f"{result}")
"#,
        "1",
    );
}

#[test]
fn test_comparison_result_in_bool_var() {
    assert_runs_with_output(
        r#"
use system.io

let a = 5
let b = 10
let lt = a < b
let gt = a > b
let eq = a == b
let r1 = if lt: 1 else: 0
let r2 = if gt: 1 else: 0
let r3 = if eq: 1 else: 0
println(f"{r1}")
println(f"{r2}")
println(f"{r3}")
"#,
        "1",
    );
}

#[test]
fn test_bool_var_chained_logic() {
    assert_runs_with_output(
        r#"
use system.io

let x = 7
let in_range = x > 0 and x < 10
let out_range = x < 0 or x > 10
let r1 = if in_range: 1 else: 0
let r2 = if out_range: 1 else: 0
println(f"{r1}")
println(f"{r2}")
"#,
        "1",
    );
}
