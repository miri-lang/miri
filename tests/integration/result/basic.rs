// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_result_ok_match() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn divide(a int, b int) Result<int, String>
    if b == 0
        return Result.Err("division by zero")
    return Result.Ok(a / b)

fn main()
    match divide(10, 2)
        Result.Ok(value): println(f"{value}")
        Result.Err(msg): println("Error: " + msg)
"#,
        "5",
    );
}

#[test]
fn test_result_err_match() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn divide(a int, b int) Result<int, String>
    if b == 0
        return Result.Err("division by zero")
    return Result.Ok(a / b)

fn main()
    match divide(10, 0)
        Result.Ok(value): println(f"{value}")
        Result.Err(msg): println("Error: " + msg)
"#,
        "Error: division by zero",
    );
}

#[test]
fn test_result_ok_construction() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn main()
    let r Result<int, String> = Result.Ok(42)
    match r
        Result.Ok(v): println(f"{v}")
        Result.Err(e): println(e)
"#,
        "42",
    );
}

#[test]
fn test_result_err_construction() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn main()
    let r Result<int, String> = Result.Err("something went wrong")
    match r
        Result.Ok(v): println(f"{v}")
        Result.Err(e): println(e)
"#,
        "something went wrong",
    );
}

#[test]
fn test_result_returned_from_function() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn parse_positive(n int) Result<int, String>
    if n < 0
        return Result.Err("negative number")
    return Result.Ok(n * 2)

fn main()
    match parse_positive(5)
        Result.Ok(v): println(f"ok:{v}")
        Result.Err(e): println(f"err:{e}")
    match parse_positive(-3)
        Result.Ok(v): println(f"ok:{v}")
        Result.Err(e): println(f"err:{e}")
"#,
        "ok:10\nerr:negative number",
    );
}
