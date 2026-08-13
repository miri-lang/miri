// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

/// Criterion 1: A generic enum with a managed payload and an UNUSED match-arm binding
/// does not leak.
#[test]
fn test_result_string_err_unused_binding() {
    assert_runs(
        r#"
fn make() Result<int, String>
    let s = "bo" + "om"
    return Result.Err(s)

match make()
    Result.Ok(v): println("ok")
    Result.Err(m): println("err")
"#,
    );
}

/// Criterion 2a: A generic enum with a managed payload that IS used still does not
/// leak and does not double-free.
#[test]
fn test_result_string_ok_used() {
    assert_runs_with_output(
        r#"
use system.io.{print}

fn make() Result<String, int>
    return Result.Ok("hello")

match make()
    Result.Ok(v): println(v)
    Result.Err(e): println("err")
"#,
        "hello",
    );
}

/// Criterion 2b: Result.Err with used payload
#[test]
fn test_result_string_err_used() {
    assert_runs_with_output(
        r#"
use system.io.{print}

fn make() Result<int, String>
    return Result.Err("error message")

match make()
    Result.Ok(v): println("ok")
    Result.Err(e): println(e)
"#,
        "error message",
    );
}

/// Criterion 2c: Option with used managed payload
#[test]
fn test_option_string_some_used() {
    assert_runs_with_output(
        r#"
use system.io.{print}

fn make() String?
    return "value"

match make()
    Some(v): println(v)
    None: println("none")
"#,
        "value",
    );
}

/// Criterion 3: A user-defined generic enum (Box2<T>) behaves the same as Result/Option
/// — no stdlib special-casing anywhere.
#[test]
fn test_user_generic_enum_unused() {
    assert_runs(
        r#"
public enum Box2<T>
    Full(T)
    Empty

fn make() Box2<String>
    return Box2.Full("data")

match make()
    Box2.Full(x): println("full")
    Box2.Empty: println("empty")
"#,
    );
}

/// Criterion 3b: user-defined generic enum with payload used
#[test]
fn test_user_generic_enum_used() {
    assert_runs_with_output(
        r#"
public enum Box2<T>
    Full(T)
    Empty

fn make() Box2<String>
    return Box2.Full("data")

match make()
    Box2.Full(x): println(x)
    Box2.Empty: println("empty")
"#,
        "data",
    );
}

/// Criterion 4: A NON-generic enum with a managed payload keeps working,
/// both used and unused.
#[test]
fn test_non_generic_enum_unused() {
    assert_runs(
        r#"
public enum MyError
    Bad(String)
    Other

fn make() MyError
    let s = "bo" + "om"
    return MyError.Bad(s)

match make()
    MyError.Bad(x): println("bad")
    MyError.Other: println("other")
"#,
    );
}

/// Criterion 4b: non-generic enum, payload used
#[test]
fn test_non_generic_enum_used() {
    assert_runs_with_output(
        r#"
public enum MyError
    Bad(String)
    Other

fn make() MyError
    return MyError.Bad("error")

match make()
    MyError.Bad(x): println(x)
    MyError.Other: println("other")
"#,
        "error",
    );
}

/// Criterion 5: A generic enum with a SCALAR payload (Result<float, E>, Box2<int>)
/// is unaffected — no spurious DecRef on a non-managed payload.
#[test]
fn test_result_float_scalar_unused() {
    assert_runs(
        r#"
fn make() Result<float, String>
    return Result.Ok(3.14)

match make()
    Result.Ok(v): println("ok")
    Result.Err(e): println("err")
"#,
    );
}

/// Criterion 5b: generic enum with scalar error type
#[test]
fn test_result_int_err_scalar() {
    assert_runs(
        r#"
fn make() Result<String, int>
    return Result.Err(42)

match make()
    Result.Ok(v): println("ok")
    Result.Err(e): println("err")
"#,
    );
}

/// Criterion 6a: nested generic enum - Result<[String], E>
#[test]
fn test_result_list_string_unused() {
    assert_runs(
        r#"
use system.collections.list

fn make() Result<[String], int>
    let list = List<String>()
    return Result.Ok(list)

fn main()
    match make()
        Result.Ok(v): println("ok")
        Result.Err(e): println("err")
"#,
    );
}

/// Criterion 6b: nested generic enum - Box2<Box2<String>>
#[test]
fn test_user_nested_generic_enum() {
    assert_runs(
        r#"
public enum Box2<T>
    Full(T)
    Empty

fn make() Box2<Box2<String>>
    return Box2.Full(Box2.Full("nested"))

fn main()
    let x = make()
    println("ok")
"#,
    );
}
