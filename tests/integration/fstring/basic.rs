// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_fstring_expression() {
    assert_runs_with_output(
        r#"
use system.io

print(f"{2 + 3 * 4}")
"#,
        "14",
    );
}

#[test]
fn test_fstring_empty() {
    assert_runs_with_output(
        r#"
use system.io

print(f"")
"#,
        "",
    );
}

#[test]
fn test_fstring_no_interpolation() {
    assert_runs_with_output(
        r#"
use system.io

print(f"just a plain string")
"#,
        "just a plain string",
    );
}

#[test]
fn test_fstring_same_variable_twice() {
    assert_runs_with_output(
        r#"
use system.io

let x = 5
print(f"{x} + {x} = {x + x}")
"#,
        "5 + 5 = 10",
    );
}
