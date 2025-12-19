// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_inline_struct_simple_members() {
    parser_test(
        "
struct Point: x int, y int
",
        vec![struct_statement(
            identifier("Point"),
            None,
            vec![
                struct_member("x", typ(Type::Int)),
                struct_member("y", typ(Type::Int)),
            ],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_block_struct_simple_members() {
    parser_test(
        "
struct Point
    x int
    y int
",
        vec![struct_statement(
            identifier("Point"),
            None,
            vec![
                struct_member("x", typ(Type::Int)),
                struct_member("y", typ(Type::Int)),
            ],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_struct_with_complex_member_types() {
    parser_test(
        "
struct UserProfile
    id string
    aliases [string]?
    preferences {string: bool}
",
        vec![struct_statement(
            identifier("UserProfile"),
            None,
            vec![
                struct_member("id", typ(Type::String)),
                struct_member("aliases", null_typ(Type::List(Box::new(typ(Type::String))))),
                struct_member(
                    "preferences",
                    typ(Type::Map(
                        Box::new(typ(Type::String)),
                        Box::new(typ(Type::Boolean)),
                    )),
                ),
            ],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_struct_with_single_member() {
    parser_test(
        "struct Wrapper: value float",
        vec![struct_statement(
            identifier("Wrapper"),
            None,
            vec![struct_member("value", typ(Type::Float))],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_error_struct_missing_name() {
    parser_error_test(
        "struct: x int",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: ":".to_string(),
        },
    );
}

#[test]
fn test_error_struct_missing_colon_or_indent() {
    parser_error_test(
        "struct Point x int",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "either a colon for inline structs or an indentation for block structs"
                .to_string(),
            found: "identifier".to_string(),
        },
    );
}

#[test]
fn test_error_struct_member_missing_type() {
    parser_error_test(
        "struct Point: x, y int",
        &SyntaxErrorKind::MissingStructMemberType,
    );
}

#[test]
fn test_error_struct_trailing_comma_inline() {
    parser_error_test(
        "struct Point: x int,",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "end of file".to_string(),
        },
    );
}

#[test]
fn test_private_struct() {
    parser_test(
        "private struct Point: x int",
        vec![struct_statement(
            identifier("Point"),
            None,
            vec![struct_member("x", typ(Type::Int))],
            MemberVisibility::Private,
        )],
    );
}

#[test]
fn test_generic_struct() {
    parser_test(
        "struct Optional<T>: value T?",
        vec![struct_statement(
            identifier("Optional"),
            Some(vec![generic_type("T", None)]),
            vec![struct_member(
                "value",
                null_typ(Type::Custom("T".into(), None)),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_struct_with_multiple_generic_parameters() {
    parser_test(
        "struct Pair<K, V>: key K, value V",
        vec![struct_statement(
            identifier("Pair"),
            Some(vec![generic_type("K", None), generic_type("V", None)]),
            vec![
                struct_member("key", typ(Type::Custom("K".into(), None))),
                struct_member("value", typ(Type::Custom("V".into(), None))),
            ],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_generic_struct_with_constraint() {
    parser_test(
        "struct Node<T extends Equatable>: value T",
        vec![struct_statement(
            identifier("Node"),
            Some(vec![generic_type_with_kind(
                "T",
                Some(Box::new(typ(Type::Custom("Equatable".into(), None)))),
                TypeDeclarationKind::Extends,
            )]),
            vec![struct_member("value", typ(Type::Custom("T".into(), None)))],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_struct_with_nested_generic_member() {
    parser_test(
        "struct Container<T>: items list<Optional<T>>",
        vec![struct_statement(
            identifier("Container"),
            Some(vec![generic_type("T", None)]),
            vec![struct_member(
                "items",
                typ(Type::List(Box::new(typ(Type::Custom(
                    "Optional".into(),
                    Some(vec![typ(Type::Custom("T".into(), None))]),
                ))))),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_error_on_empty_struct() {
    run_parser_error_tests(
        vec![
            "struct Empty:",
            "struct Empty\n    \n",
            "
struct Empty
    // This struct has no members
",
        ],
        &SyntaxErrorKind::MissingStructMembers,
    );
}
