// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_match_expression_basic() {
    let source = "
let x = 1
let res = match x
    1: 'one'
    2: 'two'
    default: 'other'
";
    type_checker_test(source);
}

#[test]
fn test_match_expression_type_mismatch() {
    let source = "
let x = 1
    let res = match x
        1: 'one'
        2: 2
";
    type_checker_error_test(source, "Match branch types mismatch");
}

#[test]
fn test_match_pattern_variable_binding() {
    let source = "
let x = 10
let res = match x
    val if val > 5: 'large'
    default: 'small'
";
    type_checker_vars_type_test(source, vec![("res", type_string())]);
}

#[test]
fn test_match_pattern_type_mismatch() {
    let source = "
let x = 1
match x
    'one': 'one'
    2: 'two'
";
    type_checker_error_test(source, "Pattern type mismatch");
}

#[test]
fn test_match_variable_leak() {
    // 'val' is bound inside the match arm. It should NOT be visible outside.
    let source = "
let x = 10
match x
    val: val

let y = val
";
    type_checker_error_test(source, "Undefined variable: val");
}

#[test]
fn test_match_variable_scope_shadowing() {
    // 'val' inside match should shadow outer 'val', but not affect it after match.
    // We use different types to verify.
    // Outer val is String. Inner val is Int (from matching 10).
    // If inner val leaks, 'z' would be Int. If not, 'z' is String.
    // We check if 'z' is String.
    let source = "
let val = \"outer\"
match 10
    val: val

let z = val
";
    type_checker_vars_type_test(source, vec![("z", type_string())]);
}

#[test]
fn test_match_multiple_branches_scope() {
    // Each branch should have its own scope.
    // 'a' in first branch, 'b' in second.
    // Neither should leak.
    let source = "
match 10
    a: a
    b: b

let x = a
";
    type_checker_error_test(source, "Undefined variable: a");
}

#[test]
fn test_match_nested_inline() {
    let source = "
let x = 1
let y = 2
let res = match x
    1: match y: 2: 'nested', default: 'other'
    default: 'outer'
";
    type_checker_vars_type_test(source, vec![("res", type_string())]);
}

#[test]
fn test_match_multiple_patterns() {
    let source = "
let code = 200
let msg = match code
    200 | 201 | 204: 'Success'
    404: 'Not Found'
    default: 'Unknown'
";
    type_checker_vars_type_test(source, vec![("msg", type_string())]);
}

#[test]
fn test_match_multiple_patterns_mismatch() {
    let source = "
let code = 200
match code
    200 | 'str': 'Success'
    default: 'Unknown'
";
    type_checker_error_test(source, "Pattern type mismatch");
}

#[test]
fn test_match_tuple_pattern() {
    let source = "
let point = (0, 0)
let msg = match point
    (0, 0): 'origin'
    (x, 0): 'on x-axis'
    default: 'other'
";
    type_checker_vars_type_test(source, vec![("msg", type_string())]);
}

#[test]
fn test_match_tuple_pattern_mismatch() {
    let source = "
let point = (0, 0)
match point
    (0, 'str'): 'origin'
    default: 'other'
";
    type_checker_error_test(source, "Pattern type mismatch");
}

#[test]
fn test_match_tuple_pattern_arity_mismatch() {
    let source = "
let point = (0, 0)
match point
    (0, 0, 0): 'origin'
    default: 'other'
";
    type_checker_error_test(source, "Tuple pattern length mismatch");
}

#[test]
fn test_match_regex_pattern() {
    let source = "
let text = \"123\"
let msg = match text
    re\"^\\d+$\": 'digits'
    re\"^[a-z]+$\": 'letters'
    default: 'other'
";
    type_checker_vars_type_test(source, vec![("msg", type_string())]);
}

#[test]
fn test_match_regex_pattern_mismatch() {
    let source = "
let num = 123
match num
    re\"^\\d+$\": 'digits'
    default: 'other'
";
    type_checker_error_test(source, "Regex pattern requires string subject");
}

#[test]
fn test_match_enum_exhaustive() {
    let source = "
enum Color
    Red
    Green
    Blue

let c = Color.Red
match c
    Color.Red: 'red'
    Color.Green: 'green'
    Color.Blue: 'blue'
";
    type_checker_test(source);
}

#[test]
fn test_match_enum_non_exhaustive() {
    let source = "
enum Color
    Red
    Green
    Blue

let c = Color.Red
match c
    Color.Red: 'red'
    Color.Green: 'green'
";
    type_checker_error_test(source, "Non-exhaustive match");
}

#[test]
fn test_match_enum_default() {
    let source = "
enum Color
    Red
    Green
    Blue

let c = Color.Red
match c
    Color.Red: 'red'
    default: 'other'
";
    type_checker_test(source);
}

#[test]
fn test_match_enum_variable_binding() {
    let source = "
enum Color
    Red
    Green
    Blue

let c = Color.Red
match c
    Color.Red: 'red'
    other: 'other'
";
    type_checker_test(source);
}
