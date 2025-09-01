// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_inline_struct_simple_members() {
    parser_test("
struct Point: x int, y int
", vec![
        struct_statement(
            identifier("Point"),
            vec![
                struct_member("x", typ(Type::Int)),
                struct_member("y", typ(Type::Int))
            ],
            MemberVisibility::Public
        )
    ]);
}

#[test]
fn test_block_struct_simple_members() {
    parser_test("
struct Point
    x int
    y int
", vec![
        struct_statement(
            identifier("Point"),
            vec![
                struct_member("x", typ(Type::Int)),
                struct_member("y", typ(Type::Int))
            ],
            MemberVisibility::Public
        )
    ]);
}

#[test]
fn test_struct_with_complex_member_types() {
    parser_test("
struct UserProfile
    id string
    aliases [string]?
    preferences {string: bool}
", vec![
        struct_statement(
            identifier("UserProfile"),
            vec![
                struct_member("id", typ(Type::String)),
                struct_member("aliases", null_typ(Type::List(Box::new(typ(Type::String))))),
                struct_member("preferences", typ(Type::Map(Box::new(typ(Type::String)), Box::new(typ(Type::Boolean)))))
            ],
            MemberVisibility::Public
        )
    ]);
}

#[test]
fn test_struct_with_single_member() {
    parser_test("struct Wrapper: value float", vec![
        struct_statement(
            identifier("Wrapper"),
            vec![struct_member("value", typ(Type::Float))],
            MemberVisibility::Public
        )
    ]);
}

#[test]
fn test_empty_block_struct() {
    parser_error_test("
struct Empty
    // This struct has no members
", &SyntaxErrorKind::UnexpectedToken {
        expected: "an indentation for block structs".to_string(),
        found: "end of file".to_string(),
    });
}

#[test]
fn test_error_struct_missing_name() {
    parser_error_test(
        "struct: x int",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: ":".to_string(),
        }
    );
}

#[test]
fn test_error_struct_missing_colon_or_indent() {
    parser_error_test(
        "struct Point x int",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "either a colon for inline structs or an indentation for block structs".to_string(),
            found: "identifier".to_string(),
        }
    );
}

#[test]
fn test_error_struct_member_missing_type() {
    parser_error_test(
        "struct Point: x, y int",
        &SyntaxErrorKind::MissingStructMemberType
    );
}

#[test]
fn test_error_struct_trailing_comma_inline() {
    parser_error_test(
        "struct Point: x int,",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "end of file".to_string(),
        }
    );
}

#[test]
fn test_private_struct() {
    parser_test("private struct Point: x int", vec![
        struct_statement(
            identifier("Point"),
            vec![struct_member("x", typ(Type::Int))],
            MemberVisibility::Private
        )
    ]);
}
