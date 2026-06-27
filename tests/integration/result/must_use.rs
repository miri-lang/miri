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
        "must be used",
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
        "must be used",
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
        "must be used",
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
