// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use std::vec;

use super::utils::{parser_error_test, parser_test};
use miri::ast::factory::{
    block, boolean_literal, enum_statement, enum_value, expression_statement, function_declaration,
    identifier, return_statement, type_bool, type_expr_non_null, type_expr_option, type_int,
    type_list, type_map, type_string,
};
use miri::ast::{FunctionProperties, MemberVisibility};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_inline_enum_simple_values() {
    parser_test(
        "
enum Colors: Red, Green, Blue
",
        vec![enum_statement(
            identifier("Colors"),
            None,
            vec![
                enum_value("Red", vec![]),
                enum_value("Green", vec![]),
                enum_value("Blue", vec![]),
            ],
            vec![],
            MemberVisibility::Public,
            false,
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
            None,
            vec![
                enum_value("Red", vec![]),
                enum_value("Green", vec![]),
                enum_value("Blue", vec![]),
            ],
            vec![],
            MemberVisibility::Public,
            false,
        )],
    );
}

#[test]
fn test_inline_enum_with_typed_values() {
    parser_test(
        "
enum Message: Write(String), Move(int, int)
",
        vec![enum_statement(
            identifier("Message"),
            None,
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
            vec![],
            MemberVisibility::Public,
            false,
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
            None,
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
            vec![],
            MemberVisibility::Public,
            false,
        )],
    );
}

#[test]
fn test_enum_with_single_value() {
    parser_test(
        "enum Status: Ok",
        vec![enum_statement(
            identifier("Status"),
            None,
            vec![enum_value("Ok", vec![])],
            vec![],
            MemberVisibility::Public,
            false,
        )],
    );
}

#[test]
fn test_enum_with_complex_value_types() {
    parser_test(
        "
enum Data: Point([int]?), Config({String: bool})
",
        vec![enum_statement(
            identifier("Data"),
            None,
            vec![
                enum_value("Point", vec![type_expr_option(type_list(type_int()))]),
                enum_value(
                    "Config",
                    vec![type_expr_non_null(type_map(type_string(), type_bool()))],
                ),
            ],
            vec![],
            MemberVisibility::Public,
            false,
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
            None,
            vec![enum_value("V", vec![type_expr_non_null(type_int())])],
            vec![],
            MemberVisibility::Public,
            false,
        )],
    );
}

#[test]
fn test_enum_visibility_modifiers() {
    parser_test(
        "public enum Color: Red",
        vec![enum_statement(
            identifier("Color"),
            None,
            vec![enum_value("Red", vec![])],
            vec![],
            MemberVisibility::Public,
            false,
        )],
    );

    parser_error_test(
        "protected enum E: V",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "public or private visibility".to_string(),
            found: "protected (only valid for class members)".to_string(),
        },
    );
    parser_test(
        "private enum E: V",
        vec![enum_statement(
            identifier("E"),
            None,
            vec![enum_value("V", vec![])],
            vec![],
            MemberVisibility::Private,
            false,
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

#[test]
fn test_enum_with_method() {
    parser_test(
        "
enum Status
    Active
    Inactive

    fn is_active() bool
        return true
",
        vec![enum_statement(
            identifier("Status"),
            None,
            vec![enum_value("Active", vec![]), enum_value("Inactive", vec![])],
            vec![function_declaration(
                "is_active",
                None,
                vec![],
                Some(Box::new(type_expr_non_null(type_bool()))),
                block(vec![return_statement(Some(Box::new(boolean_literal(
                    true,
                ))))]),
                FunctionProperties {
                    visibility: MemberVisibility::Public,
                    ..Default::default()
                },
            )],
            MemberVisibility::Public,
            false,
        )],
    );
}

#[test]
fn test_must_use_enum() {
    parser_test(
        "must_use enum Outcome: Ok, Err",
        vec![enum_statement(
            identifier("Outcome"),
            None,
            vec![enum_value("Ok", vec![]), enum_value("Err", vec![])],
            vec![],
            MemberVisibility::Public,
            true,
        )],
    );
}

#[test]
fn test_enum_with_public_method() {
    parser_test(
        "
enum Toggle
    On
    Off

    public fn is_on() bool
        return false
",
        vec![enum_statement(
            identifier("Toggle"),
            None,
            vec![enum_value("On", vec![]), enum_value("Off", vec![])],
            vec![function_declaration(
                "is_on",
                None,
                vec![],
                Some(Box::new(type_expr_non_null(type_bool()))),
                block(vec![return_statement(Some(Box::new(boolean_literal(
                    false,
                ))))]),
                FunctionProperties {
                    visibility: MemberVisibility::Public,
                    ..Default::default()
                },
            )],
            MemberVisibility::Public,
            false,
        )],
    );
}

#[test]
fn test_enum_expression_body_method() {
    parser_test(
        "
enum Flag
    Set
    Unset

    fn value() int
        42
",
        vec![enum_statement(
            identifier("Flag"),
            None,
            vec![enum_value("Set", vec![]), enum_value("Unset", vec![])],
            vec![function_declaration(
                "value",
                None,
                vec![],
                Some(Box::new(type_expr_non_null(type_int()))),
                block(vec![expression_statement(
                    miri::ast::factory::int_literal_expression(42),
                )]),
                FunctionProperties {
                    visibility: MemberVisibility::Public,
                    ..Default::default()
                },
            )],
            MemberVisibility::Public,
            false,
        )],
    );
}
