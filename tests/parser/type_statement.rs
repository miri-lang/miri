// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test, type_map_expr};
use miri::ast::factory::{
    generic_type, type_custom, type_declaration, type_expr_non_null, type_expr_option, type_int,
    type_statement, type_string,
};
use miri::ast::types::TypeDeclarationKind;
use miri::ast::{opt_expr, MemberVisibility};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_type_alias_statement() {
    parser_test(
        "
type MyInt is int
",
        vec![type_statement(
            vec![type_declaration(
                "MyInt",
                None,
                TypeDeclarationKind::Is,
                opt_expr(type_expr_non_null(type_int())),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_type_alias_complex() {
    parser_test(
        "
type UserMap is {String: User?}
",
        vec![type_statement(
            vec![type_declaration(
                "UserMap",
                None,
                TypeDeclarationKind::Is,
                opt_expr(type_expr_non_null(type_map_expr(
                    type_expr_non_null(type_string()),
                    type_expr_option(type_custom("User", None)),
                ))),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_type_parameter_unconstrained() {
    parser_test(
        "
type T, U
",
        vec![type_statement(
            vec![
                type_declaration("T", None, TypeDeclarationKind::None, None),
                type_declaration("U", None, TypeDeclarationKind::None, None),
            ],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_type_parameter_constrained() {
    parser_test(
        "
type T extends SomeClass
",
        vec![type_statement(
            vec![type_declaration(
                "T",
                None,
                TypeDeclarationKind::Extends,
                opt_expr(type_expr_non_null(type_custom("SomeClass", None))),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_type_parameter_list_mixed() {
    parser_test(
        "
type T, U extends Serializable, X implements IGraph
",
        vec![type_statement(
            vec![
                type_declaration("T", None, TypeDeclarationKind::None, None),
                type_declaration(
                    "U",
                    None,
                    TypeDeclarationKind::Extends,
                    opt_expr(type_expr_non_null(type_custom("Serializable", None))),
                ),
                type_declaration(
                    "X",
                    None,
                    TypeDeclarationKind::Implements,
                    opt_expr(type_expr_non_null(type_custom("IGraph", None))),
                ),
            ],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_error_type_statement_missing_keyword() {
    parser_error_test(
        "type T SomeClass",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "is, implements, includes or extends".to_string(),
            found: "identifier".to_string(),
        },
    );
}

#[test]
fn test_error_type_statement_trailing_comma() {
    parser_error_test(
        "type T,",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "end of file".to_string(),
        },
    );
}

#[test]
fn test_error_type_statement_missing_identifier() {
    parser_error_test(
        "type is int",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "is".to_string(),
        },
    );
}

#[test]
fn test_protected_type_alias() {
    parser_error_test(
        "protected type MyInt is int",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "public or private visibility".to_string(),
            found: "protected".to_string(),
        },
    );
}

#[test]
fn test_generic_type_alias() {
    parser_test(
        "type Optional<T> is T?",
        vec![type_statement(
            vec![type_declaration(
                "Optional",
                Some(vec![generic_type("T", None)]),
                TypeDeclarationKind::Is,
                opt_expr(type_expr_option(type_custom("T", None))),
            )],
            MemberVisibility::Public,
        )],
    );
}
