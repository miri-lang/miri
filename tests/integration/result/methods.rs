// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_is_ok_true() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn main()
    let r Result<int, String> = Result.Ok(1)
    println(f"{r.is_ok()}")
"#,
        "true",
    );
}

#[test]
fn test_is_ok_false() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn main()
    let r Result<int, String> = Result.Err("fail")
    println(f"{r.is_ok()}")
"#,
        "false",
    );
}

#[test]
fn test_is_err_true() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn main()
    let r Result<int, String> = Result.Err("fail")
    println(f"{r.is_err()}")
"#,
        "true",
    );
}

#[test]
fn test_is_err_false() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn main()
    let r Result<int, String> = Result.Ok(42)
    println(f"{r.is_err()}")
"#,
        "false",
    );
}

#[test]
fn test_unwrap_or_ok() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn main()
    let r Result<int, String> = Result.Ok(7)
    println(f"{r.unwrap_or(0)}")
"#,
        "7",
    );
}

#[test]
fn test_unwrap_or_err() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn main()
    let r Result<int, String> = Result.Err("bad")
    println(f"{r.unwrap_or(-1)}")
"#,
        "-1",
    );
}

#[test]
fn test_match_extracts_ok_value() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn main()
    let r Result<int, String> = Result.Ok(99)
    match r
        Result.Ok(value): println(f"{value}")
        Result.Err(_): println("err")
"#,
        "99",
    );
}

#[test]
fn test_match_extracts_err_value() {
    assert_runs_with_output(
        r#"
use system.io
use system.result

fn main()
    let r Result<int, String> = Result.Err("the error")
    match r
        Result.Ok(_): println("ok")
        Result.Err(msg): println(msg)
"#,
        "the error",
    );
}
