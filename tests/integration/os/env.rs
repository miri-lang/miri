// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_env_get_unset_variable() {
    assert_runs_with_output(
        r#"
use system.result
use system.os

fn main()
    let env = Env()
    match env.get("_MIRI_TEST_VAR_THAT_DOES_NOT_EXIST_12345")
        Some(v): println("found")
        None: println("not found")
"#,
        "not found",
    );
}

#[test]
fn test_env_get_set_variable() {
    assert_runs_with_output(
        r#"
use system.result
use system.os

fn main()
    let env = Env()
    match env.set("_MIRI_TEST_VAR", "testvalue")
        Result.Ok(_): println("set ok")
        Result.Err(_): println("set failed")
    match env.get("_MIRI_TEST_VAR")
        Some(v): println(v)
        None: println("not found")
"#,
        "set ok\ntestvalue",
    );
}

#[test]
fn test_env_set_returns_replacement_info() {
    assert_runs_with_output(
        r#"
use system.result
use system.os

fn main()
    let env = Env()
    match env.set("_MIRI_TEST_VAR_2", "first")
        Result.Ok(replaced): println(f"first: {replaced}")
        Result.Err(_): println("error")
    match env.set("_MIRI_TEST_VAR_2", "second")
        Result.Ok(replaced): println(f"second: {replaced}")
        Result.Err(_): println("error")
"#,
        "first: false\nsecond: true",
    );
}

#[test]
fn test_env_set_empty_string() {
    assert_runs_with_output(
        r#"
use system.result
use system.os

fn main()
    let env = Env()
    match env.set("_MIRI_TEST_VAR_3", "")
        Result.Ok(_): println("set ok")
        Result.Err(_): println("set failed")
    match env.get("_MIRI_TEST_VAR_3")
        Some(v): println(f"value is empty string: {v.length() == 0}")
        None: println("unset")
"#,
        "set ok\nvalue is empty string: true",
    );
}

/// Proves an empty-valued variable reads back as `Some("")` while an unset one
/// reads back as `None`.
#[test]
fn test_env_unset_vs_empty_string() {
    assert_runs_with_output(
        r#"
use system.result
use system.os

fn main()
    let env = Env()
    match env.set("_MIRI_TEST_EMPTY", "")
        Result.Ok(_): println("ok")
        Result.Err(_): println("err")
    match env.get("_MIRI_TEST_EMPTY")
        Some(_): println("empty var is Some")
        None: println("empty var is None")
    match env.get("_MIRI_TEST_UNSET")
        Some(_): println("unset var is Some")
        None: println("unset var is None")
"#,
        "ok\nempty var is Some\nunset var is None",
    );
}

#[test]
fn test_env_set_invalid_name_with_equals() {
    assert_runs_with_output(
        r#"
use system.result
use system.os

fn main()
    let env = Env()
    match env.set("_MIRI=TEST", "value")
        Result.Ok(_): println("unexpectedly succeeded")
        Result.Err(EnvError.InvalidName(_)): println("invalid name")
        Result.Err(_): println("other error")
"#,
        "invalid name",
    );
}

#[test]
fn test_env_set_value_containing_nul_is_rejected() {
    assert_runs_with_output(
        r#"
use system.result
use system.os

fn main()
    let env = Env()
    match env.set("_MIRI_NUL_VALUE", "a\0b")
        Result.Ok(_): println("unexpectedly succeeded")
        Result.Err(EnvError.InvalidValue(_)): println("invalid value")
        Result.Err(_): println("other error")
"#,
        "invalid value",
    );
}

#[test]
fn test_env_set_invalid_name_empty() {
    assert_runs_with_output(
        r#"
use system.result
use system.os

fn main()
    let env = Env()
    match env.set("", "value")
        Result.Ok(_): println("unexpectedly succeeded")
        Result.Err(EnvError.InvalidName(_)): println("invalid name")
        Result.Err(_): println("other error")
"#,
        "invalid name",
    );
}
