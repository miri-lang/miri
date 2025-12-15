// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_conditional_expression_basic() {
    let source = "
    let x = 10 if true else 20
    ";
    check_success(source);
}

#[test]
fn test_conditional_expression_condition_not_boolean() {
    let source = "
    let x = 10 if 1 else 20
    ";
    check_error(source, "Conditional condition must be a boolean");
}

#[test]
fn test_conditional_expression_branch_mismatch() {
    let source = "
    let x = 10 if true else 'hello'
    ";
    check_error(source, "Conditional branches must have the same type");
}

#[test]
fn test_conditional_expression_no_else_void() {
    let source = "
fn foo()
    return

let x = foo() if true
";
    check_success(source);
}

#[test]
fn test_conditional_expression_no_else_non_void() {
    let source = "
    let x = 10 if true
    ";
    check_error(source, "Conditional expression without else branch must return Void");
}
