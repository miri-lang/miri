// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::{factory::type_result, Type};

#[test]
fn test_result_ok_inference() {
    let source = "
let r = Ok(10)
r
    ";
    check_expr_type(source, type_result(Type::Int, Type::Void));
}

#[test]
fn test_result_err_inference() {
    let source = "
let r = Err(\"error\")
r
    ";
    check_expr_type(source, type_result(Type::Void, Type::String));
}

#[test]
fn test_result_ok_assignment() {
    let source = "
let r result<int, string> = Ok(10)
r
    ";
    check_expr_type(source, type_result(Type::Int, Type::String));
}

#[test]
fn test_result_err_assignment() {
    let source = "
let r result<int, string> = Err(\"fail\")
r
    ";
    check_expr_type(source, type_result(Type::Int, Type::String));
}

#[test]
fn test_result_ok_type_mismatch() {
    let source = "
let r result<int, string> = Ok(\"wrong\")
    ";
    check_error(source, "Type mismatch");
}

#[test]
fn test_result_err_type_mismatch() {
    let source = "
let r result<int, string> = Err(10)
    ";
    check_error(source, "Type mismatch");
}

#[test]
fn test_result_methods_is_ok() {
    let source = "
let r = Ok(10)
r.is_ok()
    ";
    check_expr_type(source, Type::Boolean);
}

#[test]
fn test_result_methods_is_err() {
    let source = "
let r = Err(\"error\")
r.is_err()
    ";
    check_expr_type(source, Type::Boolean);
}

#[test]
fn test_result_methods_unwrap() {
    let source = "
let r = Ok(10)
r.unwrap()
    ";
    check_expr_type(source, Type::Int);
}

#[test]
fn test_result_methods_unwrap_on_err() {
    let source = "
let r = Err(\"error\")
r.unwrap()
    ";
    // unwrap on Err returns Void because Ok type is Void
    check_expr_type(source, Type::Void);
}

#[test]
fn test_result_methods_unwrap_typed() {
    let source = "
let r result<int, string> = Err(\"error\")
r.unwrap()
    ";
    // unwrap on typed Result returns the Ok type (Int)
    check_expr_type(source, Type::Int);
}

#[test]
fn test_nested_result() {
    let source = "
let r result<result<int, string>, bool> = Ok(Ok(10))
r
    ";
    check_expr_type(
        source,
        type_result(type_result(Type::Int, Type::String), Type::Boolean),
    );
}

#[test]
fn test_nested_result_unwrap() {
    let source = "
let r result<result<int, string>, bool> = Ok(Ok(10))
r.unwrap().unwrap()
    ";
    check_expr_type(source, Type::Int);
}

#[test]
fn test_ok_argument_count() {
    let source = "
let r = Ok(1, 2)
    ";
    check_error(source, "Too many positional arguments");
}

#[test]
fn test_err_argument_count() {
    let source = "
let r = Err()
    ";
    check_error(source, "Missing argument for parameter 'error'");
}

#[test]
fn test_result_invalid_member() {
    let source = "
let r = Ok(10)
r.foo
    ";
    check_error(source, "does not have members");
}

#[test]
fn test_result_match_bind() {
    let source = "
let r = Ok(10)
match r
    x: x.unwrap()
    ";
    check_expr_type(source, Type::Int);
}

#[test]
fn test_result_match_shadow_ok_fails() {
    let source = "
let r = Ok(10)
match r
    Ok: Ok.unwrap()
    ";
    // Ok resolves to the constructor function, which doesn't have unwrap()
    check_error(source, "does not have members");
}

#[test]
fn test_result_custom_struct_error() {
    let source = "
struct MyError
    code int
    message string

let e = MyError(code: 404, message: \"Not Found\")
let r result<int, MyError> = Err(e)
r
    ";
    check_expr_type(
        source,
        type_result(Type::Int, Type::Custom("MyError".to_string(), None)),
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
    check_error(source, "Type mismatch");
}

#[test]
fn test_result_custom_error_return() {
    let source = "
struct MyError
    msg string

fn fail() result<int, MyError>
    return Err(MyError(msg: \"fail\"))

fail()
    ";
    check_expr_type(
        source,
        type_result(Type::Int, Type::Custom("MyError".to_string(), None)),
    );
}
