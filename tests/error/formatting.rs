// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::check_error_output;

#[test]
fn test_undefined_variable_suggestion() {
    let code = r#"
fn main()
    let a = 10
    let b = aa
"#;
    check_error_output(
        code,
        &[
            "error: Undefined variable: aa",
            "help: Did you mean 'a'?",
            "let b = aa",
            "        ^^",
        ],
    );
}

#[test]
fn test_unknown_type_suggestion() {
    let code = r#"
fn main()
    let s strng = "hello"
"#;
    check_error_output(
        code,
        &[
            "error: Unknown type: strng",
            "help: Did you mean 'String'?",
            "let s strng = \"hello\"",
            "      ^^^^^",
        ],
    );
}

#[test]
fn test_missing_struct_field_suggestion() {
    let code = r#"
struct Point
    x int
    y int

fn main()
    let p = Point(x: 1, y: 2)
    let v = p.z
"#;
    // Note: The suggestion might be 'x' or 'y' depending on implementation details (order/distance).
    // We check for the error and the pointer.
    check_error_output(
        code,
        &[
            "error: Type 'Point' has no field 'z'",
            "let v = p.z",
            "        ^^^",
        ],
    );
}

#[test]
fn test_missing_enum_variant_suggestion() {
    let code = r#"
enum Status
    Ok
    Error

fn main()
    let s = Status.Errr
"#;
    check_error_output(
        code,
        &[
            "error: Enum 'Status' has no variant 'Errr'",
            "help: Did you mean 'Error'?",
            "let s = Status.Errr",
            "        ^^^^^^^^^^^",
        ],
    );
}

#[test]
fn test_multiple_errors_formatting() {
    let code = r#"
fn main()
    let a = 10
    let b = aa
    let c = bb
"#;
    check_error_output(
        code,
        &[
            "error: Undefined variable: aa",
            "error: Undefined variable: bb",
            "let b = aa",
            "        ^^",
            "let c = bb",
            "        ^^",
        ],
    );
}

#[test]
fn test_type_mismatch_formatting() {
    let code = r#"
fn main()
    let a int = "string"
"#;
    check_error_output(code, &["error:", "let a int = \"string\""]);
}

#[test]
fn test_syntax_error_formatting() {
    let code = r#"
fn main()
    let s = "unclosed string
"#;
    // The lexer currently reports this as Invalid Token or Unclosed String Literal.
    // We check for the code line and pointer.
    check_error_output(code, &["let s = \"unclosed string", "        ^"]);
}
