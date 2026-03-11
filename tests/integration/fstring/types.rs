// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_fstring_int() {
    assert_runs_with_output(
        r#"
use system.io

let x = 42
print(f"{x}")
"#,
        "42",
    );
}

#[test]
fn test_fstring_float() {
    assert_runs_with_output(
        r#"
use system.io

let x = 3.14
print(f"{x}")
"#,
        "3.14",
    );
}

#[test]
fn test_fstring_bool() {
    assert_runs_with_output(
        r#"
use system.io

let x = true
print(f"{x}")
"#,
        "true",
    );
}

#[test]
fn test_fstring_mixed_types() {
    assert_runs_with_output(
        r#"
use system.io

let name = "Miri"
let version = 1
let active = true
print(f"{name} v{version} active={active}")
"#,
        "Miri v1 active=true",
    );
}

#[test]
fn test_fstring_nested_expressions() {
    assert_runs_with_output(
        r#"
use system.io

let a = 10
let b = 20
print(f"{a} + {b} = {a + b}")
"#,
        "10 + 20 = 30",
    );
}
