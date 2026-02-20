// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_for_loop_range() {
    type_checker_test("for i in 0..10: 1");
}

#[test]
fn test_for_loop_range_variable_type() {
    // i should be int
    type_checker_test("for i in 0..10: let x = i");
}

#[test]
fn test_for_loop_range_variable_type_mismatch() {
    type_checker_error_test(
        "for i in 0..10: let x = i + \"s\"",
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_for_loop_range_mismatch() {
    type_checker_error_test("for i in 0..\"10\": 1", "Range types mismatch");
}

#[test]
fn test_for_loop_not_iterable() {
    type_checker_error_test("for i in true: 1", "Type bool is not iterable");
}

#[test]
fn test_break_in_loop() {
    type_checker_test("for i in 0..10: break");
    type_checker_test("while true: break");
}

#[test]
fn test_break_outside_loop() {
    type_checker_error_test("break", "Break statement outside of loop");
}

#[test]
fn test_continue_in_loop() {
    type_checker_test("for i in 0..10: continue");
    type_checker_test("while true: continue");
}

#[test]
fn test_continue_outside_loop() {
    type_checker_error_test("continue", "Continue statement outside of loop");
}

#[test]
fn test_nested_loops_break() {
    type_checker_test("for i in 0..10: while true: break");
}

#[test]
fn test_nested_loops_break_outside() {
    type_checker_error_test(
        "for i in 0..10: while true: fn foo(): break",
        "Break statement outside of loop",
    );
}

#[test]
fn test_break_in_function_in_loop() {
    // for i in 0..10 { fn foo() { break } } -> Should be error
    // Function body should not inherit loop depth from the outer scope.
    type_checker_error_test(
        "for i in 0..10: fn foo(): break",
        "Break statement outside of loop",
    );
}

#[test]
fn test_while_condition_type() {
    type_checker_test("while true: 1");
    type_checker_error_test("while 1: 1", "While condition must be a boolean");
}

#[test]
fn test_if_condition_type() {
    type_checker_test("if true: 1");
    type_checker_error_test("if 1: 1", "If condition must be a boolean");
}

#[test]
fn test_for_loop_list() {
    type_checker_vars_type_test(
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
    type_checker_vars_type_test(
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
    type_checker_vars_type_test(
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
    type_checker_vars_type_test(
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
    type_checker_vars_type_test(
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
    type_checker_vars_type_test(
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
    type_checker_error_test(
        "
for i in 0..10: 1
let x = i
",
        "Undefined variable: i",
    );
}

#[test]
fn test_return_in_loop() {
    type_checker_test(
        "
fn foo()
    for i in 0..10
        return
",
    );
}

#[test]
fn test_break_continue_in_if_in_loop() {
    type_checker_test(
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
    type_checker_test(
        "
while true
    break
    let x = 1
",
    );
}

#[test]
fn test_unreachable_code_after_return() {
    type_checker_test(
        "
fn foo()
    return
    let x = 1
",
    );
}

#[test]
fn test_deeply_nested_loops() {
    type_checker_test(
        "
for i in 0..5
    for j in 0..5
        for k in 0..5
            for l in 0..5
                for m in 0..5
                    let x = i + j + k + l + m
",
    );
}

#[test]
fn test_deeply_nested_while() {
    type_checker_test(
        "
while true
    while true
        while true
            while true
                break
            break
        break
    break
",
    );
}

#[test]
fn test_nested_if_else_chain() {
    type_checker_test(
        "
let x = 5
if x > 10
    1
else if x > 5
    2
else if x > 0
    3
else
    4
",
    );
}

#[test]
fn test_loop_with_complex_conditions() {
    type_checker_test(
        "
let a = true
let b = false
while (a and not b) or (b and not a)
    break
",
    );
}

#[test]
fn test_for_with_many_iterations() {
    type_checker_test(
        "
for i in 0..1000
    let x = i * i
",
    );
}

#[test]
fn test_nested_break_continue_pattern() {
    type_checker_test(
        "
for i in 0..10
    if i < 3
        continue
    if i > 7
        break
    for j in 0..10
        if j == i
            break
        if j < i
            continue
",
    );
}

#[test]
fn test_mixed_loops_deeply_nested() {
    type_checker_test(
        "
for i in 0..5
    while i < 10
        for j in 0..3
            while j < 5
                break
            break
        break
",
    );
}

#[test]
fn test_return_in_nested_loop() {
    type_checker_test(
        "
fn find_value() int
    for i in 0..10
        for j in 0..10
            if i * j == 42
                return i * j
    return -1

find_value()
",
    );
}

#[test]
fn test_loop_variable_shadowing_nested() {
    type_checker_test(
        "
for i in 0..5
    let i = \"shadowed\"
    for j in 0..3
        let j = true
",
    );
}

#[test]
fn test_error_deeply_nested_break_outside() {
    type_checker_error_test(
        "
for i in 0..10
    fn inner()
        break
",
        "Break statement outside of loop",
    );
}
