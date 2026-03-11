// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_string_interpolation() {
    assert_runs_with_output(
        r#"
use system.io

let name = "Miri"
print(f"Hello, {name}!")
"#,
        "Hello, Miri!",
    );
}

#[test]
fn test_string_interpolation_expression() {
    assert_runs_with_output(
        r#"
use system.io

let x = 5
print(f"5 + 3 = {x + 3}")
"#,
        "5 + 3 = 8",
    );
}

#[test]
fn test_formatted_string_escape_newline() {
    assert_runs_with_output(
        r#"
use system.io

let x = 42
print(f"value:\t{x}\n")
"#,
        "value:\t42\n",
    );
}
