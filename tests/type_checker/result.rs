// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_result_ok_inference() {
    let source = "
use system.result
let r = Ok(10)
    ";
    type_checker_vars_type_test(source, vec![("r", type_result(type_int(), type_void()))]);
}

#[test]
fn test_result_err_inference() {
    let source = "
use system.result
let r = Err(\"error\")
    ";
    type_checker_vars_type_test(source, vec![("r", type_result(type_void(), type_string()))]);
}

#[test]
fn test_result_ok_assignment() {
    let source = "
use system.result
let r result<int, String> = Ok(10)
    ";
    type_checker_vars_type_test(source, vec![("r", type_result(type_int(), type_string()))]);
}

#[test]
fn test_result_err_assignment() {
    let source = "
use system.result
let r result<int, String> = Err(\"fail\")
    ";
    type_checker_vars_type_test(source, vec![("r", type_result(type_int(), type_string()))]);
}

#[test]
fn test_result_ok_type_mismatch() {
    let source = "
use system.result
let r result<int, String> = Ok(\"wrong\")
    ";
    type_checker_error_test(source, "Type mismatch");
}

#[test]
fn test_result_err_type_mismatch() {
    let source = "
use system.result
let r result<int, String> = Err(10)
    ";
    type_checker_error_test(source, "Type mismatch");
}

#[test]
fn test_result_methods_is_ok() {
    let source = "
use system.result
let r = Ok(10)
r.is_ok()
    ";
    type_checker_expr_type_test(source, type_bool());
}

#[test]
fn test_result_methods_is_err() {
    let source = "
use system.result
let r = Err(\"error\")
r.is_err()
    ";
    type_checker_expr_type_test(source, type_bool());
}

#[test]
fn test_result_methods_unwrap_removed() {
    // unwrap() was removed from Result to satisfy the no-stdlib-panics rule.
    // Users must use `match` or `unwrap_or(...)` instead.
    let source = "
use system.result
let r = Ok(10)
r.unwrap()
    ";
    type_checker_error_test(source, "has no method");
}

#[test]
fn test_nested_result() {
    let source = "
use system.result
let r result<result<int, String>, bool> = Ok(Ok(10))
    ";
    type_checker_vars_type_test(
        source,
        vec![(
            "r",
            type_result(type_result(type_int(), type_string()), type_bool()),
        )],
    );
}

#[test]
fn test_nested_result_unwrap_or_extracts() {
    let source = "
use system.result
let r result<result<int, String>, bool> = Ok(Ok(10))
let inner = r.unwrap_or(Ok(0))
inner.unwrap_or(0)
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_ok_argument_count() {
    let source = "
use system.result
let r = Ok(1, 2)
    ";
    type_checker_error_test(source, "Too many positional arguments");
}

#[test]
fn test_err_argument_count() {
    let source = "
use system.result
let r = Err()
    ";
    type_checker_error_test(source, "Missing argument for parameter 'error'");
}

#[test]
fn test_result_invalid_member() {
    let source = "
use system.result
let r = Ok(10)
r.foo
    ";
    type_checker_error_test(source, "has no method");
}

#[test]
fn test_result_match_bind() {
    let source = "
use system.result
let r = Ok(10)
match r
    x: x.unwrap_or(0)
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_result_match_shadow_ok_fails() {
    let source = "
use system.result
let r = Ok(10)
match r
    Ok: Ok.unwrap_or(0)
    ";
    // Ok resolves to the constructor function, which doesn't have unwrap_or()
    type_checker_error_test(source, "does not have members");
}

#[test]
fn test_result_custom_struct_error() {
    let source = "
use system.result
struct MyError
    code int
    message String

let e = MyError(code: 404, message: \"Not Found\")
let r result<int, MyError> = Err(e)
    ";
    type_checker_vars_type_test(
        source,
        vec![("r", type_result(type_int(), type_custom("MyError", None)))],
    );
}

#[test]
fn test_result_custom_struct_error_mismatch() {
    let source = "
use system.result
struct MyError
    code int

struct OtherError
    code int

let e = OtherError(code: 500)
let r result<int, MyError> = Err(e)
    ";
    type_checker_error_test(source, "Type mismatch");
}

#[test]
fn test_result_custom_error_return() {
    let source = "
use system.result
struct MyError
    msg String

fn fail() result<int, MyError>
    return Err(MyError(msg: \"fail\"))

let r = fail()
    ";
    type_checker_vars_type_test(
        source,
        vec![("r", type_result(type_int(), type_custom("MyError", None)))],
    );
}
