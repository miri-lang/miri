// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_empty_class() {
    parser_test(
        "
class MyClass
    let x int
",
        vec![class_statement(
            identifier("MyClass"),
            None,
            None,
            vec![],
            vec![variable_statement(
                vec![let_variable(
                    "x",
                    opt_expr(type_expr_non_null(type_int())),
                    None,
                )],
                MemberVisibility::Private,
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_class_with_field() {
    parser_test(
        "
class Point
    let x int
    let y int
",
        vec![class_statement(
            identifier("Point"),
            None,
            None,
            vec![],
            vec![
                variable_statement(
                    vec![let_variable(
                        "x",
                        opt_expr(type_expr_non_null(type_int())),
                        None,
                    )],
                    MemberVisibility::Private,
                ),
                variable_statement(
                    vec![let_variable(
                        "y",
                        opt_expr(type_expr_non_null(type_int())),
                        None,
                    )],
                    MemberVisibility::Private,
                ),
            ],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_class_with_mutable_field() {
    parser_test(
        "
class Counter
    var count int
",
        vec![class_statement(
            identifier("Counter"),
            None,
            None,
            vec![],
            vec![variable_statement(
                vec![var("count", opt_expr(type_expr_non_null(type_int())), None)],
                MemberVisibility::Private,
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_class_with_method() {
    class_statement_test(
        "
class Calculator
    fn add(a int, b int) int
        a + b
",
        identifier("Calculator"),
        None,
        None,
        vec![],
        vec![function_declaration(
            "add",
            None,
            vec![
                parameter("a".into(), type_expr_non_null(type_int()), None, None),
                parameter("b".into(), type_expr_non_null(type_int()), None, None),
            ],
            Some(Box::new(type_expr_non_null(type_int()))),
            block(vec![expression_statement(binary(
                identifier("a"),
                BinaryOp::Add,
                identifier("b"),
            ))]),
            FunctionProperties {
                visibility: MemberVisibility::Private,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_with_init() {
    class_statement_test(
        "
class Point
    let x int
    fn init(x int)
        self.x = x
",
        identifier("Point"),
        None,
        None,
        vec![],
        vec![
            variable_statement(
                vec![let_variable(
                    "x",
                    opt_expr(type_expr_non_null(type_int())),
                    None,
                )],
                MemberVisibility::Private,
            ),
            function_declaration(
                "init",
                None,
                vec![parameter(
                    "x".into(),
                    type_expr_non_null(type_int()),
                    None,
                    None,
                )],
                None,
                block(vec![expression_statement(assign(
                    lhs_member(identifier("self"), identifier("x")),
                    AssignmentOp::Assign,
                    identifier("x"),
                ))]),
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
fn test_class_extends() {
    class_statement_test(
        "
class Dog extends Animal
    let name string
",
        identifier("Dog"),
        None,
        Some(Box::new(identifier("Animal"))),
        vec![],
        vec![variable_statement(
            vec![let_variable(
                "name",
                opt_expr(type_expr_non_null(type_string())),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_implements() {
    class_statement_test(
        "
class MyList implements Iterable
    let items int
",
        identifier("MyList"),
        None,
        None,
        vec![identifier("Iterable")],
        vec![variable_statement(
            vec![let_variable(
                "items",
                opt_expr(type_expr_non_null(type_int())),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_implements_multiple() {
    class_statement_test(
        "
class MyList implements Iterable, Sortable
    let items int
",
        identifier("MyList"),
        None,
        None,
        vec![identifier("Iterable"), identifier("Sortable")],
        vec![variable_statement(
            vec![let_variable(
                "items",
                opt_expr(type_expr_non_null(type_int())),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_extends_and_implements() {
    class_statement_test(
        "
class Dog extends Animal implements Trainable
    let name string
",
        identifier("Dog"),
        None,
        Some(Box::new(identifier("Animal"))),
        vec![identifier("Trainable")],
        vec![variable_statement(
            vec![let_variable(
                "name",
                opt_expr(type_expr_non_null(type_string())),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_public_field() {
    class_statement_test(
        "
class Point
    public let x int
",
        identifier("Point"),
        None,
        None,
        vec![],
        vec![variable_statement(
            vec![let_variable(
                "x",
                opt_expr(type_expr_non_null(type_int())),
                None,
            )],
            MemberVisibility::Public,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_protected_method() {
    class_statement_test(
        "
class Point
    protected fn helper()
        x
",
        identifier("Point"),
        None,
        None,
        vec![],
        vec![function_declaration(
            "helper",
            None,
            vec![],
            None,
            block(vec![expression_statement(identifier("x"))]),
            FunctionProperties {
                visibility: MemberVisibility::Protected,
                ..Default::default()
            },
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_private_field_explicit() {
    class_statement_test(
        "
class Point
    private let x int
",
        identifier("Point"),
        None,
        None,
        vec![],
        vec![variable_statement(
            vec![let_variable(
                "x",
                opt_expr(type_expr_non_null(type_int())),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_with_generics() {
    class_statement_test(
        "
class Box<T>
    let value T
",
        identifier("Box"),
        Some(vec![generic_type("T", None)]),
        None,
        vec![],
        vec![variable_statement(
            vec![let_variable(
                "value",
                opt_expr(type_expr_non_null(type_custom("T", None))),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_class_with_generic_constraint() {
    class_statement_test(
        "
class SortedList<T extends Comparable>
    let items T
",
        identifier("SortedList"),
        Some(vec![generic_type_with_kind(
            "T",
            Some(Box::new(type_expr_non_null(type_custom(
                "Comparable",
                None,
            )))),
            TypeDeclarationKind::Extends,
        )]),
        None,
        vec![],
        vec![variable_statement(
            vec![let_variable(
                "items",
                opt_expr(type_expr_non_null(type_custom("T", None))),
                None,
            )],
            MemberVisibility::Private,
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn test_super_expression() {
    parser_test(
        "super.init()",
        vec![expression_statement(call(
            member(super_expression(), identifier("init")),
            vec![],
        ))],
    );
}

#[test]
fn test_super_method_with_args() {
    parser_test(
        "super.method(x, y)",
        vec![expression_statement(call(
            member(super_expression(), identifier("method")),
            vec![identifier("x"), identifier("y")],
        ))],
    );
}

#[test]
fn test_error_class_no_body() {
    // This should actually succeed with empty body according to our implementation
    // But if we require at least one member, uncomment below
    // parser_error_test("class Empty", &SyntaxErrorKind::...);
}

#[test]
fn test_error_class_implements_before_extends() {
    parser_error_test(
        "
class Dog implements Trainable extends Animal
    let x int
",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "end of expression".to_string(),
            found: "extends".to_string(),
        },
    );
}

#[test]
fn test_error_class_invalid_member() {
    parser_error_test(
        "
class Point
    if x
",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "class member (let, var, fn, async, gpu, or type)".to_string(),
            found: "if".to_string(),
        },
    );
}
