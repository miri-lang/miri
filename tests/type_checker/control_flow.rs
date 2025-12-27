// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_for_loop_range() {
    check_success("for i in 0..10: 1");
}

#[test]
fn test_for_loop_range_variable_type() {
    // i should be Int
    check_success("for i in 0..10: let x = i");
}

#[test]
fn test_for_loop_range_variable_type_mismatch() {
    check_error(
        "for i in 0..10: let x = i + \"s\"",
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_for_loop_range_mismatch() {
    check_error("for i in 0..\"10\": 1", "Range types mismatch");
}

#[test]
fn test_for_loop_not_iterable() {
    check_error("for i in true: 1", "Type Boolean is not iterable");
}

#[test]
fn test_break_in_loop() {
    check_success("for i in 0..10: break");
    check_success("while true: break");
}

#[test]
fn test_break_outside_loop() {
    check_error("break", "Break statement outside of loop");
}

#[test]
fn test_continue_in_loop() {
    check_success("for i in 0..10: continue");
    check_success("while true: continue");
}

#[test]
fn test_continue_outside_loop() {
    check_error("continue", "Continue statement outside of loop");
}

#[test]
fn test_nested_loops_break() {
    check_success("for i in 0..10: while true: break");
}

#[test]
fn test_nested_loops_break_outside() {
    check_error(
        "for i in 0..10: while true: fn foo(): break",
        "Break statement outside of loop",
    );
}

#[test]
fn test_break_in_function_in_loop() {
    // for i in 0..10 { fn foo() { break } } -> Should be error
    // Function body should not inherit loop depth from the outer scope.
    check_error(
        "for i in 0..10: fn foo(): break",
        "Break statement outside of loop",
    );
}

#[test]
fn test_while_condition_type() {
    check_success("while true: 1");
    check_error("while 1: 1", "While condition must be a boolean");
}

#[test]
fn test_if_condition_type() {
    check_success("if true: 1");
    check_error("if 1: 1", "If condition must be a boolean");
}

#[test]
fn test_for_loop_list() {
    check_vars_type(
        "
let l = [1, 2, 3]
for x in l
    let y = x
",
        vec![("y", type_int())],
    );
}

#[test]
fn test_for_loop_string() {
    check_vars_type(
        "
for c in \"hello\"
    let y = c
",
        vec![("y", type_string())],
    );
}

#[test]
fn test_for_loop_map() {
    // Iterating over map yields tuples (key, value)
    check_vars_type(
        "
let m = {\"a\": 1}
for entry in m
    let k = entry[0]
    let v = entry[1]
",
        vec![("k", type_string()), ("v", type_int())],
    );
}

#[test]
fn test_for_loop_set() {
    check_vars_type(
        "
let s = {1, 2, 3}
for x in s
    let y = x
",
        vec![("y", type_int())],
    );
}

#[test]
fn test_for_loop_destructuring_map() {
    check_vars_type(
        "
let m = {\"a\": 1}
for k, v in m
    let key = k
    let val = v
",
        vec![("key", type_string()), ("val", type_int())],
    );
}

#[test]
fn test_for_loop_destructuring_tuple_list() {
    check_vars_type(
        "
let l = [(1, \"a\"), (2, \"b\")]
for n, s in l
    let num = n
    let str = s
",
        vec![("num", type_int()), ("str", type_string())],
    );
}

#[test]
fn test_scope_leak() {
    check_error(
        "
for i in 0..10: 1
let x = i
",
        "Undefined variable: i",
    );
}

#[test]
fn test_return_in_loop() {
    check_success(
        "
fn foo()
    for i in 0..10
        return
",
    );
}

#[test]
fn test_break_continue_in_if_in_loop() {
    check_success(
        "
for i in 0..10
    if i > 5
        break
    else
        continue
",
    );
}

#[test]
fn test_unreachable_code_after_break() {
    // Type checker might not catch this as error, but it shouldn't crash
    check_success(
        "
while true
    break
    let x = 1
",
    );
}

#[test]
fn test_unreachable_code_after_return() {
    check_success(
        "
fn foo()
    return
    let x = 1
",
    );
}
