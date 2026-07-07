// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_fstring_expression() {
    assert_runs_with_output(
        r#"

print(f"{2 + 3 * 4}")
"#,
        "14",
    );
}

#[test]
fn test_fstring_empty() {
    assert_runs_with_output(
        r#"

print(f"")
"#,
        "",
    );
}

#[test]
fn test_fstring_no_interpolation() {
    assert_runs_with_output(
        r#"

print(f"just a plain string")
"#,
        "just a plain string",
    );
}

#[test]
fn test_fstring_escaped_braces() {
    // A backslash-escaped brace inside an f-string is a literal brace, not an
    // interpolation delimiter, and the backslash must be stripped from the
    // output. Escaped braces coexist with real interpolations.
    assert_runs_with_output(
        r#"
let x = 5
print(f"\{x is {x}\}")
"#,
        "{x is 5}",
    );
}

#[test]
fn test_fstring_escaped_braces_only() {
    assert_runs_with_output(
        r#"
print(f"\{literal\}")
"#,
        "{literal}",
    );
}

#[test]
fn test_fstring_same_variable_twice() {
    assert_runs_with_output(
        r#"

let x = 5
print(f"{x} + {x} = {x + x}")
"#,
        "5 + 5 = 10",
    );
}
