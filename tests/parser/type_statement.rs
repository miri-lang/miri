// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_type_alias_statement() {
    parser_test("
type MyInt is int
", vec![
        type_statement(vec![
            type_declaration(
                "MyInt",
                TypeDeclarationKind::Is,
                opt_expr(typ(Type::Int))
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_type_alias_complex() {
    parser_test("
type UserMap is {string: User?}
", vec![
        type_statement(vec![
            type_declaration(
                "UserMap",
                TypeDeclarationKind::Is,
                opt_expr(typ(Type::Map(
                    Box::new(typ(Type::String)),
                    Box::new(null_typ(Type::Custom("User".into(), None)))
                )))
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_type_parameter_unconstrained() {
    parser_test("
type T, U
", vec![
        type_statement(vec![
            type_declaration("T", TypeDeclarationKind::None, None),
            type_declaration("U", TypeDeclarationKind::None, None)
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_type_parameter_constrained() {
    parser_test("
type T extends SomeClass
", vec![
        type_statement(vec![
            type_declaration(
                "T",
                TypeDeclarationKind::Extends,
                opt_expr(typ(Type::Custom("SomeClass".into(), None)))
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_type_parameter_list_mixed() {
    parser_test("
type T, U extends Serializable, X implements IGraph
", vec![
        type_statement(vec![
            type_declaration("T", TypeDeclarationKind::None, None),
            type_declaration(
                "U",
                TypeDeclarationKind::Extends,
                opt_expr(typ(Type::Custom("Serializable".into(), None)))
            ),
            type_declaration(
                "X",
                TypeDeclarationKind::Implements,
                opt_expr(typ(Type::Custom("IGraph".into(), None)))
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_error_type_statement_missing_keyword() {
    parser_error_test(
        "type T SomeClass",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "is, implements, includes or extends".to_string(),
            found: "identifier".to_string(),
        }
    );
}

#[test]
fn test_error_type_statement_trailing_comma() {
    parser_error_test(
        "type T,",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "end of file".to_string(),
        }
    );
}

#[test]
fn test_error_type_statement_missing_identifier() {
    parser_error_test(
        "type is int",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "is".to_string(),
        }
    );
}

#[test]
fn test_protected_type_alias() {
    parser_test("protected type MyInt is int", vec![
        type_statement(
            vec![type_declaration("MyInt", TypeDeclarationKind::Is, opt_expr(typ(Type::Int)))],
            MemberVisibility::Protected
        )
    ]);
}
