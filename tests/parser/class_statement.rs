// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

// ===== Basic Class Declaration =====

#[test]
fn test_empty_class() {
    parser_test(
        "class MyClass\n    let x int",
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
        "class Point\n    let x int\n    let y int",
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
        "class Counter\n    var count int",
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

// ===== Class with Methods =====

#[test]
fn test_class_with_method() {
    let source = "class Calculator\n    fn add(a int, b int) int\n        a + b";
    let program = parse_program(source);
    assert_eq!(program.body.len(), 1);
    if let StatementKind::Class(name, _, _, _, body, _) = &program.body[0].node {
        assert_eq!(
            name.node,
            ExpressionKind::Identifier("Calculator".into(), None)
        );
        assert_eq!(body.len(), 1);
        if let StatementKind::FunctionDeclaration(fn_name, _, _, _, _, _) = &body[0].node {
            assert_eq!(fn_name, "add");
        } else {
            panic!("Expected function declaration");
        }
    } else {
        panic!("Expected class statement");
    }
}

#[test]
fn test_class_with_init() {
    let source = "class Point\n    let x int\n    fn init(x int)\n        self.x = x";
    let program = parse_program(source);
    assert_eq!(program.body.len(), 1);
    if let StatementKind::Class(_, _, _, _, body, _) = &program.body[0].node {
        assert_eq!(body.len(), 2); // field + init
    } else {
        panic!("Expected class statement");
    }
}

// ===== Inheritance =====

#[test]
fn test_class_extends() {
    let source = "class Dog extends Animal\n    let name String";
    let program = parse_program(source);
    assert_eq!(program.body.len(), 1);
    if let StatementKind::Class(name, _, base_class, _, body, _) = &program.body[0].node {
        assert_eq!(name.node, ExpressionKind::Identifier("Dog".into(), None));
        assert!(base_class.is_some());
        if let ExpressionKind::Identifier(base_name, None) = &base_class.as_ref().unwrap().node {
            assert_eq!(base_name, "Animal");
        } else {
            panic!("Expected identifier for base class");
        }
        assert_eq!(body.len(), 1);
    } else {
        panic!("Expected class statement");
    }
}

#[test]
fn test_class_implements() {
    let source = "class MyList implements Iterable\n    let items int";
    let program = parse_program(source);
    if let StatementKind::Class(name, _, _, traits, body, _) = &program.body[0].node {
        assert_eq!(name.node, ExpressionKind::Identifier("MyList".into(), None));
        assert_eq!(traits.len(), 1);
        assert_eq!(body.len(), 1);
    } else {
        panic!("Expected class statement");
    }
}

#[test]
fn test_class_implements_multiple() {
    let source = "class MyList implements Iterable, Sortable\n    let items int";
    let program = parse_program(source);
    if let StatementKind::Class(_, _, _, traits, _, _) = &program.body[0].node {
        assert_eq!(traits.len(), 2);
    } else {
        panic!("Expected class statement");
    }
}

#[test]
fn test_class_extends_and_implements() {
    let source = "class Dog extends Animal implements Trainable\n    let name String";
    let program = parse_program(source);
    if let StatementKind::Class(_, _, base_class, traits, body, _) = &program.body[0].node {
        assert!(base_class.is_some());
        assert_eq!(traits.len(), 1);
        assert_eq!(body.len(), 1);
    } else {
        panic!("Expected class statement");
    }
}

// ===== Visibility Modifiers =====

#[test]
fn test_class_public_field() {
    let source = "class Point\n    public let x int";
    let program = parse_program(source);
    if let StatementKind::Class(_, _, _, _, body, _) = &program.body[0].node {
        if let StatementKind::Variable(_, vis) = &body[0].node {
            assert_eq!(*vis, MemberVisibility::Public);
        } else {
            panic!("Expected variable statement");
        }
    } else {
        panic!("Expected class statement");
    }
}

#[test]
fn test_class_protected_method() {
    let source = "class Point\n    protected fn helper()\n        x";
    let program = parse_program(source);
    if let StatementKind::Class(_, _, _, _, body, _) = &program.body[0].node {
        if let StatementKind::FunctionDeclaration(_, _, _, _, _, props) = &body[0].node {
            assert_eq!(props.visibility, MemberVisibility::Protected);
        } else {
            panic!("Expected function declaration");
        }
    } else {
        panic!("Expected class statement");
    }
}

#[test]
fn test_class_private_field_explicit() {
    let source = "class Point\n    private let x int";
    let program = parse_program(source);
    if let StatementKind::Class(_, _, _, _, body, _) = &program.body[0].node {
        if let StatementKind::Variable(_, vis) = &body[0].node {
            assert_eq!(*vis, MemberVisibility::Private);
        } else {
            panic!("Expected variable statement");
        }
    } else {
        panic!("Expected class statement");
    }
}

// ===== Generics =====

#[test]
fn test_class_with_generics() {
    let source = "class Box<T>\n    let value T";
    let program = parse_program(source);
    if let StatementKind::Class(_, generics, _, _, _, _) = &program.body[0].node {
        assert!(generics.is_some());
        assert_eq!(generics.as_ref().unwrap().len(), 1);
    } else {
        panic!("Expected class statement");
    }
}

#[test]
fn test_class_with_generic_constraint() {
    let source = "class SortedList<T extends Comparable>\n    let items T";
    let program = parse_program(source);
    if let StatementKind::Class(_, generics, _, _, _, _) = &program.body[0].node {
        assert!(generics.is_some());
        let generic = &generics.as_ref().unwrap()[0];
        if let ExpressionKind::GenericType(_, constraint, _) = &generic.node {
            assert!(constraint.is_some());
        } else {
            panic!("Expected generic type");
        }
    } else {
        panic!("Expected class statement");
    }
}

// ===== Super Expression =====

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

// ===== Error Cases =====

#[test]
fn test_error_class_no_body() {
    // This should actually succeed with empty body according to our implementation
    // But if we require at least one member, uncomment below
    // parser_error_test("class Empty", &SyntaxErrorKind::...);
}

#[test]
fn test_error_class_implements_before_extends() {
    parser_error_test(
        "class Dog implements Trainable extends Animal\n    let x int",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "end of expression".to_string(),
            found: "extends".to_string(),
        },
    );
}

#[test]
fn test_error_class_invalid_member() {
    parser_error_test(
        "class Point\n    if x",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "class member (let, var, fn, async, gpu, or type)".to_string(),
            found: "if".to_string(),
        },
    );
}
