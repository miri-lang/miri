// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test, run_parser_error_tests};
use miri::ast::factory::{
    generic_type, generic_type_with_kind, identifier, struct_member, struct_statement, type_bool,
    type_custom, type_expr_non_null, type_expr_option, type_float, type_int, type_list, type_map,
    type_string,
};
use miri::ast::types::TypeDeclarationKind;
use miri::ast::MemberVisibility;
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
                struct_member("x", type_expr_non_null(type_int())),
                struct_member("y", type_expr_non_null(type_int())),
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
                struct_member("x", type_expr_non_null(type_int())),
                struct_member("y", type_expr_non_null(type_int())),
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
    id String
    aliases [String]?
    preferences {String: bool}
",
        vec![struct_statement(
            identifier("UserProfile"),
            None,
            vec![
                struct_member("id", type_expr_non_null(type_string())),
                struct_member("aliases", type_expr_option(type_list(type_string()))),
                struct_member(
                    "preferences",
                    type_expr_non_null(type_map(type_string(), type_bool())),
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
            vec![struct_member("value", type_expr_non_null(type_float()))],
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
            vec![struct_member("x", type_expr_non_null(type_int()))],
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
                type_expr_option(type_custom("T", None)),
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
                struct_member("key", type_expr_non_null(type_custom("K", None))),
                struct_member("value", type_expr_non_null(type_custom("V", None))),
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
                Some(Box::new(type_expr_non_null(type_custom("Equatable", None)))),
                TypeDeclarationKind::Extends,
            )]),
            vec![struct_member(
                "value",
                type_expr_non_null(type_custom("T", None)),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_struct_with_nested_generic_member() {
    parser_test(
        "struct Container<T>: items List<Optional<T>>",
        vec![struct_statement(
            identifier("Container"),
            Some(vec![generic_type("T", None)]),
            vec![struct_member(
                "items",
                type_expr_non_null(type_list(type_custom(
                    "Optional",
                    Some(vec![type_expr_non_null(type_custom("T", None))]),
                ))),
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
