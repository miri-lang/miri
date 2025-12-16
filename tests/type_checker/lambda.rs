// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::Type;

#[test]
fn test_lambda_inference_simple() {
    let source = "
let f = fn(a int) int: a + 1
f(1)
    ";
    check_expr_type(source, Type::Int);
}

#[test]
fn test_lambda_inference_implicit_return() {
    let source = "
let f = fn(a int): a + 1
f(1)
    ";
    check_expr_type(source, Type::Int);
}

#[test]
fn test_lambda_inference_block_body() {
    let source = "
let f = fn(a int)
    let b = 10
    a + b

f(5)
    ";
    check_expr_type(source, Type::Int);
}

#[test]
fn test_lambda_inference_void() {
    let source = "
let f = fn(a int)
    let b = a + 1

f(1)
    ";
    // The call returns Void
    check_expr_type(source, Type::Void);
}

#[test]
fn test_lambda_explicit_return_mismatch() {
    let source = "
let f = fn(a int) int
    return true
    ";
    check_error(source, "Invalid return type: expected Int, got Boolean");
}

#[test]
fn test_lambda_implicit_return_mismatch() {
    let source = "
let f = fn(a int) int: true
    ";
    check_error(source, "Invalid return type: expected Int, got Boolean");
}

#[test]
fn test_lambda_inferred_return_mismatch() {
    let source = "
let f = fn(a int)
    if a > 0: return 1
    return true
    ";
    check_error(source, "Incompatible return types in lambda: Int and Boolean");
}

#[test]
fn test_lambda_as_argument() {
    let source = "
fn apply(f fn(int) int, x int) int
    return f(x)

apply(fn(a int): a * 2, 10)
    ";
    check_expr_type(source, Type::Int);
}

#[test]
fn test_nested_lambda() {
    let source = "
let make_adder = fn(x int) fn(int) int
    return fn(y int): x + y

let add5 = make_adder(5)
add5(10)
    ";
    check_expr_type(source, Type::Int);
}
