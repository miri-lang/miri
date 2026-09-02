// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_result_must_use_ignored() {
    assert_compiler_error(
        r#"
use system.result

fn divide(a int, b int) Result<int, String>
    if b == 0
        return Result.Err("division by zero")
    return Result.Ok(a / b)

fn main()
    divide(10, 2)
"#,
        "Unused value of type",
    );
}

#[test]
fn test_result_must_use_ok_literal() {
    assert_compiler_error(
        r#"
use system.result

fn main()
    Result.Ok(42)
"#,
        "Unused value of type",
    );
}

#[test]
fn test_result_must_use_err_literal() {
    assert_compiler_error(
        r#"
use system.result

fn main()
    Result.Err("oops")
"#,
        "Unused value of type",
    );
}

#[test]
fn test_result_ok_when_bound_to_variable() {
    assert_runs(
        r#"
use system.result

fn divide(a int, b int) Result<int, String>
    if b == 0
        return Result.Err("division by zero")
    return Result.Ok(a / b)

fn main()
    let r = divide(10, 2)
    match r
        Result.Ok(_): ()
        Result.Err(_): ()
"#,
    );
}

// Acceptance criterion 1: tail if/else returning Result should compile
#[test]
fn test_tail_if_result_single_line() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn parse_qty(text String) Result<int, String>
    if text == "": Result.Err("empty")
    else: Result.Ok(3)

fn main()
    let qty = parse_qty("")
    match qty
        Result.Ok(v): println(f"qty {v}")
        Result.Err(e): println(f"error {e}")
"#,
        "error empty\n",
    );
}

// Acceptance criterion 1: tail if/else with Result, block form
#[test]
fn test_tail_if_result_block_form() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn parse_qty(text String) Result<int, String>
    if text == ""
        Result.Err("empty")
    else
        Result.Ok(3)

fn main()
    let qty = parse_qty("ok")
    match qty
        Result.Ok(v): println(f"qty {v}")
        Result.Err(e): println(f"error {e}")
"#,
        "qty 3\n",
    );
}

// Acceptance criterion 2: Nested tail if/else returning Result
#[test]
fn test_nested_tail_if_result() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn check(a int, b int) Result<int, String>
    if a == 0
        Result.Err("a is zero")
    else
        if b == 0
            Result.Err("b is zero")
        else
            Result.Ok(a + b)

fn main()
    let r1 = check(0, 5)
    let r2 = check(5, 5)
    match r1
        Result.Ok(v): println(f"r1={v}")
        Result.Err(e): println(f"r1 error: {e}")
    match r2
        Result.Ok(v): println(f"r2={v}")
        Result.Err(e): println(f"r2 error: {e}")
"#,
        "r1 error: a is zero\nr2=10\n",
    );
}

// Acceptance criterion 2: user-defined @must_use enum with tail if/else
#[test]
fn test_tail_if_custom_must_use() {
    assert_runs_with_output(
        r#"
use system.io

@must_use
enum Status
    Ok(int)
    Err(String)

fn check(code int) Status
    if code == 0
        Status.Ok(42)
    else
        Status.Err("error")

fn main()
    let s1 = check(0)
    let s2 = check(1)
    match s1
        Status.Ok(v): println(f"s1={v}")
        Status.Err(_): ()
    match s2
        Status.Ok(_): ()
        Status.Err(e): println(f"s2={e}")
"#,
        "s1=42\ns2=error\n",
    );
}

// Acceptance criterion 3a: non-tail if statement discarding Result should still error
#[test]
fn test_non_tail_if_discarding_result_errors() {
    assert_compiler_error(
        r#"
use system.result

fn main()
    if true
        Result.Ok(42)
    else
        Result.Err("bad")
    println("after")
"#,
        "Unused value of type",
    );
}

// Acceptance criterion 3b: discard before tail expression inside tail if should error
#[test]
fn test_discard_before_tail_in_if_errors() {
    assert_compiler_error(
        r#"
use system.result

fn check(a int) Result<int, String>
    if a == 0
        Result.Ok(1)
        Result.Err("zero")
    else
        Result.Ok(a)
"#,
        "Unused value of type",
    );
}

// Acceptance criterion 3c: tail if with no else returning Result should error
// The if is in tail position but has no else, so it cannot produce a value.
// This should report both unused value (MER_OWN_004) and missing return (MER_TYP_054).
#[test]
fn test_tail_if_no_else_result_unused_value() {
    assert_compiler_error(
        r#"
use system.result

fn check(a int) Result<int, String>
    if a == 0
        Result.Ok(1)
"#,
        "Unused value of type",
    );
}

// Acceptance criterion 3c: tail if with no else should also error on missing return
#[test]
fn test_tail_if_no_else_result_missing_return() {
    assert_compiler_error(
        r#"
use system.result

fn check(a int) Result<int, String>
    if a == 0
        Result.Ok(1)
"#,
        "Missing return statement",
    );
}
