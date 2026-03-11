// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_null_coalesce_some() {
    assert_runs_with_output(
        r#"
use system.io

let x int? = 42
println(f"{x ?? 0}")
"#,
        "42",
    );
}

#[test]
fn test_null_coalesce_none() {
    assert_runs_with_output(
        r#"
use system.io

let x int? = None
println(f"{x ?? 99}")
"#,
        "99",
    );
}

#[test]
fn test_null_coalesce_string_some() {
    assert_runs_with_output(
        r#"
use system.io

let s String? = "hello"
println(s ?? "default")
"#,
        "hello",
    );
}

#[test]
fn test_null_coalesce_string_none() {
    assert_runs_with_output(
        r#"
use system.io

let s String? = None
println(s ?? "default")
"#,
        "default",
    );
}

#[test]
fn test_null_coalesce_with_function_call() {
    assert_runs_with_output(
        r#"
use system.io

fn f() int?
    return None

println(f"{f() ?? 0}")
"#,
        "0",
    );
}

#[test]
fn test_null_coalesce_with_some_constructor() {
    assert_runs_with_output(
        r#"
use system.io

let x = Some(42)
println(f"{x ?? 0}")
"#,
        "42",
    );
}
