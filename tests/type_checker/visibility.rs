// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::type_checker::TypeChecker;

#[test]
fn test_visibility_same_module() {
    let mut tc = TypeChecker::new();
    tc.set_current_module("A".to_string());

    // private let x = 1
    let stmt = Statement::Variable(
        vec![VariableDeclaration {
            name: "x".to_string(),
            typ: None,
            initializer: Some(Box::new(IdNode::new(
                0,
                ExpressionKind::Literal(Literal::Integer(IntegerLiteral::I32(1))),
                0..0,
            ))),
            declaration_type: VariableDeclarationType::Immutable,
        }],
        MemberVisibility::Private,
    );

    let program = Program { body: vec![stmt] };
    assert!(tc.check(&program).is_ok());

    // Access x
    // x
    let expr = IdNode::new(1, ExpressionKind::Identifier("x".to_string(), None), 0..0);
    let stmt2 = Statement::Expression(expr);
    let program2 = Program { body: vec![stmt2] };
    assert!(tc.check(&program2).is_ok());
}

#[test]
fn test_visibility_different_module() {
    let mut tc = TypeChecker::new();
    tc.set_current_module("A".to_string());

    // private let x = 1
    let stmt = Statement::Variable(
        vec![VariableDeclaration {
            name: "x".to_string(),
            typ: None,
            initializer: Some(Box::new(IdNode::new(
                0,
                ExpressionKind::Literal(Literal::Integer(IntegerLiteral::I32(1))),
                0..0,
            ))),
            declaration_type: VariableDeclarationType::Immutable,
        }],
        MemberVisibility::Private,
    );

    let program = Program { body: vec![stmt] };
    assert!(tc.check(&program).is_ok());

    // Switch module
    tc.set_current_module("B".to_string());

    // Access x
    // x
    let expr = IdNode::new(1, ExpressionKind::Identifier("x".to_string(), None), 0..0);
    let stmt2 = Statement::Expression(expr);
    let program2 = Program { body: vec![stmt2] };

    let result = tc.check(&program2);
    assert!(result.is_err());
    let errors = result.err().unwrap();
    assert!(errors[0].message.contains("Variable 'x' is not visible"));
}

#[test]
fn test_visibility_public_different_module() {
    let mut tc = TypeChecker::new();
    tc.set_current_module("A".to_string());

    // public let x = 1
    let stmt = Statement::Variable(
        vec![VariableDeclaration {
            name: "x".to_string(),
            typ: None,
            initializer: Some(Box::new(IdNode::new(
                0,
                ExpressionKind::Literal(Literal::Integer(IntegerLiteral::I32(1))),
                0..0,
            ))),
            declaration_type: VariableDeclarationType::Immutable,
        }],
        MemberVisibility::Public,
    );

    let program = Program { body: vec![stmt] };
    assert!(tc.check(&program).is_ok());

    // Switch module
    tc.set_current_module("B".to_string());

    // Access x
    let expr = IdNode::new(1, ExpressionKind::Identifier("x".to_string(), None), 0..0);
    let stmt2 = Statement::Expression(expr);
    let program2 = Program { body: vec![stmt2] };

    assert!(tc.check(&program2).is_ok());
}
