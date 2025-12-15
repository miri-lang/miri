// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::Type;

#[test]
fn test_function_declaration_and_call() {
    let source = "
fn add(a int, b int) int
    return a + b

add(1, 2)
    ";
    assert_expression_type(source, Type::Int);
}

#[test]
fn test_function_return_type_mismatch() {
    let source = "
fn foo() int
    return true
    ";
    assert_type_check_error(source, "Invalid return type: expected Int, got Boolean");
}

#[test]
fn test_function_argument_type_mismatch() {
    let source = "
fn foo(a int)
    return

foo(true)
    ";
    assert_type_check_error(source, "Type mismatch for argument 1: expected Int, got Boolean");
}

#[test]
fn test_function_argument_count_mismatch() {
    let source = "
fn foo(a int)
    return

foo(1, 2)
    ";
    assert_type_check_error(source, "Incorrect number of arguments: expected 1, got 2");
}

#[test]
fn test_void_function() {
    let source = "
fn foo()
    return

foo()
    ";
    // Just check if it passes type checking
    type_check_test(source);
}

#[test]
fn test_nested_function_calls() {
    let source = "
fn add(a int, b int) int
    return a + b

add(add(1, 2), 3)
    ";
    assert_expression_type(source, Type::Int);
}

#[test]
fn test_recursion() {
    let source = "
fn factorial(n int) int
    if n <= 1: return 1
    return n * factorial(n - 1)

factorial(5)
    ";
    assert_expression_type(source, Type::Int);
}
