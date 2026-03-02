// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_result_ok_inference() {
    let source = "
let r = Ok(10)
r
    ";
    type_checker_expr_type_test(source, type_result(type_int(), type_void()));
}

#[test]
fn test_result_err_inference() {
    let source = "
let r = Err(\"error\")
r
    ";
    type_checker_expr_type_test(source, type_result(type_void(), type_string()));
}

#[test]
fn test_result_ok_assignment() {
    let source = "
let r result<int, String> = Ok(10)
r
    ";
    type_checker_expr_type_test(source, type_result(type_int(), type_string()));
}

#[test]
fn test_result_err_assignment() {
    let source = "
let r result<int, String> = Err(\"fail\")
r
    ";
    type_checker_expr_type_test(source, type_result(type_int(), type_string()));
}

#[test]
fn test_result_ok_type_mismatch() {
    let source = "
let r result<int, String> = Ok(\"wrong\")
    ";
    type_checker_error_test(source, "Type mismatch");
}

#[test]
fn test_result_err_type_mismatch() {
    let source = "
let r result<int, String> = Err(10)
    ";
    type_checker_error_test(source, "Type mismatch");
}

#[test]
fn test_result_methods_is_ok() {
    let source = "
let r = Ok(10)
r.is_ok()
    ";
    type_checker_expr_type_test(source, type_bool());
}

#[test]
fn test_result_methods_is_err() {
    let source = "
let r = Err(\"error\")
r.is_err()
    ";
    type_checker_expr_type_test(source, type_bool());
}

#[test]
fn test_result_methods_unwrap() {
    let source = "
let r = Ok(10)
r.unwrap()
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_result_methods_unwrap_on_err() {
    let source = "
let r = Err(\"error\")
r.unwrap()
    ";
    // unwrap on Err returns Void because Ok type is Void
    type_checker_expr_type_test(source, type_void());
}

#[test]
fn test_result_methods_unwrap_typed() {
    let source = "
let r result<int, String> = Err(\"error\")
r.unwrap()
    ";
    // unwrap on typed Result returns the Ok type (Int)
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_nested_result() {
    let source = "
let r result<result<int, String>, bool> = Ok(Ok(10))
r
    ";
    type_checker_expr_type_test(
        source,
        type_result(type_result(type_int(), type_string()), type_bool()),
    );
}

#[test]
fn test_nested_result_unwrap() {
    let source = "
let r result<result<int, String>, bool> = Ok(Ok(10))
r.unwrap().unwrap()
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_ok_argument_count() {
    let source = "
let r = Ok(1, 2)
    ";
    type_checker_error_test(source, "Too many positional arguments");
}

#[test]
fn test_err_argument_count() {
    let source = "
let r = Err()
    ";
    type_checker_error_test(source, "Missing argument for parameter 'error'");
}

#[test]
fn test_result_invalid_member() {
    let source = "
let r = Ok(10)
r.foo
    ";
    type_checker_error_test(source, "does not have members");
}

#[test]
fn test_result_match_bind() {
    let source = "
let r = Ok(10)
match r
    x: x.unwrap()
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_result_match_shadow_ok_fails() {
    let source = "
let r = Ok(10)
match r
    Ok: Ok.unwrap()
    ";
    // Ok resolves to the constructor function, which doesn't have unwrap()
    type_checker_error_test(source, "does not have members");
}

#[test]
fn test_result_custom_struct_error() {
    let source = "
struct MyError
    code int
    message String

let e = MyError(code: 404, message: \"Not Found\")
let r result<int, MyError> = Err(e)
r
    ";
    type_checker_expr_type_test(
        source,
        type_result(type_int(), type_custom("MyError", None)),
    );
}

#[test]
fn test_result_custom_struct_error_mismatch() {
    let source = "
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
struct MyError
    msg String

fn fail() result<int, MyError>
    return Err(MyError(msg: \"fail\"))

fail()
    ";
    type_checker_expr_type_test(
        source,
        type_result(type_int(), type_custom("MyError", None)),
    );
}
