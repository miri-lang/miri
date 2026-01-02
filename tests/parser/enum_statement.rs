// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use std::vec;

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_inline_enum_simple_values() {
    parser_test(
        "
enum Colors: Red, Green, Blue
",
        vec![enum_statement(
            identifier("Colors"),
            vec![
                enum_value("Red", vec![]),
                enum_value("Green", vec![]),
                enum_value("Blue", vec![]),
            ],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_block_enum_simple_values() {
    parser_test(
        "
enum Colors
    Red
    Green
    Blue
",
        vec![enum_statement(
            identifier("Colors"),
            vec![
                enum_value("Red", vec![]),
                enum_value("Green", vec![]),
                enum_value("Blue", vec![]),
            ],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_inline_enum_with_typed_values() {
    parser_test(
        "
enum Message: Write(string), Move(int, int)
",
        vec![enum_statement(
            identifier("Message"),
            vec![
                enum_value("Write", vec![type_expr_non_null(type_string())]),
                enum_value(
                    "Move",
                    vec![
                        type_expr_non_null(type_int()),
                        type_expr_non_null(type_int()),
                    ],
                ),
            ],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_block_enum_with_mixed_values() {
    parser_test(
        "
enum Event
    Quit
    KeyPress(int)
    Click(int, int)
",
        vec![enum_statement(
            identifier("Event"),
            vec![
                enum_value("Quit", vec![]),
                enum_value("KeyPress", vec![type_expr_non_null(type_int())]),
                enum_value(
                    "Click",
                    vec![
                        type_expr_non_null(type_int()),
                        type_expr_non_null(type_int()),
                    ],
                ),
            ],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_enum_with_single_value() {
    parser_test(
        "enum Status: Ok",
        vec![enum_statement(
            identifier("Status"),
            vec![enum_value("Ok", vec![])],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_enum_with_complex_value_types() {
    parser_test(
        "
enum Data: Point([int]?), Config({string: bool})
",
        vec![enum_statement(
            identifier("Data"),
            vec![
                enum_value("Point", vec![type_expr_null(type_list(type_int()))]),
                enum_value(
                    "Config",
                    vec![type_expr_non_null(type_map(type_string(), type_bool()))],
                ),
            ],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_empty_block_enum() {
    parser_error_test(
        "
enum EmptyEnum
    // No values

let x = 0
",
        &SyntaxErrorKind::MissingEnumMembers,
    );
}

#[test]
fn test_error_enum_missing_name() {
    parser_error_test(
        "enum: Red, Blue",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: ":".to_string(),
        },
    );
}

#[test]
fn test_error_enum_missing_colon_or_indent() {
    parser_error_test(
        "enum Colors Red",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "either a colon for inline enums or an indentation for block enums"
                .to_string(),
            found: "identifier".to_string(),
        },
    );
}

#[test]
fn test_error_enum_empty_inline() {
    parser_error_test("enum Colors:", &SyntaxErrorKind::MissingEnumMembers);
}

#[test]
fn test_error_enum_malformed_value_type() {
    parser_test(
        "enum E: V(int,)",
        vec![enum_statement(
            identifier("E"),
            vec![enum_value("V", vec![type_expr_non_null(type_int())])],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_enum_visibility_modifiers() {
    parser_test(
        "public enum Color: Red",
        vec![enum_statement(
            identifier("Color"),
            vec![enum_value("Red", vec![])],
            MemberVisibility::Public,
        )],
    );

    parser_test(
        "protected enum E: V",
        vec![enum_statement(
            identifier("E"),
            vec![enum_value("V", vec![])],
            MemberVisibility::Protected,
        )],
    );
    parser_test(
        "private enum E: V",
        vec![enum_statement(
            identifier("E"),
            vec![enum_value("V", vec![])],
            MemberVisibility::Private,
        )],
    );
}

#[test]
fn test_enum_with_keyword_names() {
    parser_error_test(
        "enum E: if, else, match",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "if".to_string(),
        },
    );
}

#[test]
fn test_error_on_empty_block_enum() {
    parser_error_test("enum Empty: ", &SyntaxErrorKind::MissingEnumMembers);

    parser_error_test("enum Empty\n    \n", &SyntaxErrorKind::MissingEnumMembers);
}

#[test]
fn test_error_on_trailing_comma_in_inline_enum() {
    parser_error_test(
        "enum E: A, B,",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "end of file".to_string(),
        },
    );
}
