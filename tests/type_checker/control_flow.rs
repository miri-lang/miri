// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;


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
    check_error("for i in 0..10: let x = i + \"s\"", "Invalid types for arithmetic operation");
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
    check_error("for i in 0..10: while true: fn foo(): break", "Break statement outside of loop");
}

#[test]
fn test_break_in_function_in_loop() {
    // for i in 0..10 { fn foo() { break } } -> Should be error
    // Function body should not inherit loop depth from the outer scope.
    check_error("for i in 0..10: fn foo(): break", "Break statement outside of loop");
}
