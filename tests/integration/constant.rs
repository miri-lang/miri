// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_compiler_error, assert_runs_many, assert_runs_with_output};

#[test]
fn const_declaration() {
    assert_runs_many(&[
        "const x = 10",
        "const y int = 20",
        "const s String = \"hello\"",
        "const b bool = true",
    ]);
}

#[test]
fn const_print_value() {
    assert_runs_with_output(
        r#"
use system.io

const x = 42
print(f"{x}")
"#,
        "42",
    );
}

#[test]
fn const_in_expression() {
    assert_runs_with_output(
        r#"
use system.io

const x = 10
print(f"{x + 5}")
"#,
        "15",
    );
}

#[test]
fn const_multiple_declarations() {
    assert_runs_with_output(
        r#"
use system.io

const x = 10
const y = 20
print(f"{x + y}")
"#,
        "30",
    );
}

#[test]
fn const_reassignment_is_error() {
    assert_compiler_error(
        "
const x = 10
x = 20
",
        "Cannot assign to immutable variable",
    );
}
