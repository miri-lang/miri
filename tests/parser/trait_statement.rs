// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_simple_trait() {
    trait_statement_test(
        "
trait Drawable
    fn draw()
        x
",
        identifier("Drawable"),
        None,
        vec![],
        vec![function_declaration(
            "draw",
            None,
            vec![],
            None,
            block(vec![expression_statement(identifier("x"))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_with_multiple_methods() {
    trait_statement_test(
        "
trait Comparable
    fn compare(other int) int
        0
    fn equals(other int) bool
        true
",
        identifier("Comparable"),
        None,
        vec![],
        vec![
            function_declaration(
                "compare",
                None,
                vec![parameter(
                    "other".into(),
                    type_expr_non_null(type_int()),
                    None,
                    None,
                )],
                Some(Box::new(type_expr_non_null(type_int()))),
                block(vec![expression_statement(int_literal_expression(0))]),
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
            function_declaration(
                "equals",
                None,
                vec![parameter(
                    "other".into(),
                    type_expr_non_null(type_int()),
                    None,
                    None,
                )],
                Some(Box::new(type_expr_non_null(type_bool()))),
                block(vec![expression_statement(boolean_literal(true))]),
                FunctionProperties {
                    visibility: MemberVisibility::Private,
                    ..Default::default()
                },
            ),
        ],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_extends_single() {
    trait_statement_test(
        "
trait Sortable extends Comparable
    fn sort()
        x
",
        identifier("Sortable"),
        None,
        vec![identifier("Comparable")],
        vec![function_declaration(
            "sort",
            None,
            vec![],
            None,
            block(vec![expression_statement(identifier("x"))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_extends_multiple() {
    trait_statement_test(
        "
trait ReadWrite extends Readable, Writable
    fn readwrite()
        x
",
        identifier("ReadWrite"),
        None,
        vec![identifier("Readable"), identifier("Writable")],
        vec![function_declaration(
            "readwrite",
            None,
            vec![],
            None,
            block(vec![expression_statement(identifier("x"))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_with_generics() {
    trait_statement_test(
        "
trait Container<T>
    fn add(item T)
        x
",
        identifier("Container"),
        Some(vec![generic_type("T", None)]),
        vec![],
        vec![function_declaration(
            "add",
            None,
            vec![parameter(
                "item".into(),
                type_expr_non_null(type_custom("T", None)),
                None,
                None,
            )],
            None,
            block(vec![expression_statement(identifier("x"))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_trait_with_generic_constraint() {
    trait_statement_test(
        "
trait OrderedContainer<T extends Comparable>
    fn sort()
        x
",
        identifier("OrderedContainer"),
        Some(vec![generic_type_with_kind(
            "T",
            Some(Box::new(type_expr_non_null(type_custom(
                "Comparable",
                None,
            )))),
            TypeDeclarationKind::Extends,
        )]),
        vec![],
        vec![function_declaration(
            "sort",
            None,
            vec![],
            None,
            block(vec![expression_statement(identifier("x"))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_public_trait() {
    trait_statement_test(
        "
trait API
    fn call()
        x
",
        identifier("API"),
        None,
        vec![],
        vec![function_declaration(
            "call",
            None,
            vec![],
            None,
            block(vec![expression_statement(identifier("x"))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_error_trait_with_implements() {
    // Traits use extends for inheritance, not implements
    parser_error_test(
        "
trait Invalid implements Something
    fn method()
        x
",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "end of expression".to_string(),
            found: "implements".to_string(),
        },
    );
}

#[test]
fn test_error_trait_invalid_member() {
    parser_error_test(
        "
trait Invalid
    for x in y",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "class member (let, var, fn, async, gpu, or type)".to_string(),
            found: "for".to_string(),
        },
    );
}
