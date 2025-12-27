// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_lambda_inference_simple() {
    let source = "
let f = fn(a int) int: a + 1
f(1)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_lambda_inference_implicit_return() {
    let source = "
let f = fn(a int): a + 1
f(1)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_lambda_inference_block_body() {
    let source = "
let f = fn(a int)
    let b = 10
    a + b

f(5)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_lambda_inference_void() {
    let source = "
let f = fn(a int)
    let b = a + 1

f(1)
    ";
    // The call returns Void
    check_expr_type(source, type_void());
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
    check_error(
        source,
        "Incompatible return types in lambda: Int and Boolean",
    );
}

#[test]
fn test_lambda_as_argument() {
    let source = "
fn apply(f fn(int) int, x int) int
    return f(x)

apply(fn(a int): a * 2, 10)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_nested_lambda() {
    let source = "
let make_adder = fn(x int) fn(int) int
    return fn(y int): x + y

let add5 = make_adder(5)
add5(10)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_lambda_no_args() {
    let source = "
let f = fn() int: 42
f()
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_lambda_multiple_args() {
    let source = "
let add = fn(a int, b int) int: a + b
add(1, 2)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_lambda_shadowing() {
    let source = "
let x = 10
let f = fn(x int) int: x * 2
f(5)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_lambda_closure_capture() {
    let source = "
let x = 10
let f = fn() int: x
f()
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_lambda_generic() {
    let source = "
let id = fn<T>(x T) T: x
id(1)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_immediate_invocation() {
    let source = "
(fn(x int) int: x + 1)(10)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_lambda_in_list() {
    let source = "
let list = [fn(x int): x, fn(x int): x * 2]
list[0](1)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_lambda_return_type_inference_with_multiple_returns() {
    let source = "
let f = fn(x int)
    if x > 0: return 1
    return 0
f(10)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_lambda_return_type_inference_mismatch_in_branches() {
    let source = "
let f = fn(x int)
    if x > 0: return 1
    return true
    ";
    check_error(source, "Incompatible return types in lambda");
}

#[test]
fn test_nested_lambda_inference_complex() {
    let source = "
let f = fn(x int)
    let g = fn(y int)
        if y > 0: return y
        return x
    g(x)
f(10)
    ";
    check_expr_type(source, type_int());
}
