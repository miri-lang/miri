// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_println_int_type_error() {
    assert_compiler_error(
        r#"
use system.io

println(42)
"#,
        "Type mismatch",
    );
}

#[test]
fn test_println_bool_type_error() {
    assert_compiler_error(
        r#"
use system.io

println(true)
"#,
        "Type mismatch",
    );
}

#[test]
fn test_println_float_type_error() {
    assert_compiler_error(
        r#"
use system.io

println(3.14)
"#,
        "Type mismatch",
    );
}
