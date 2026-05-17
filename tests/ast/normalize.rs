// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::common::MemberVisibility;
use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::factory::{
    block, class_decl, expression_statement, for_statement, func, identifier,
    int_literal_expression, iter_obj, let_variable, list, parameter, return_statement,
    struct_member, struct_statement, type_expr_non_null, type_int, type_list, type_string, var,
    variable_statement,
};
use miri::ast::normalize::{normalize, normalize_type};
use miri::ast::pattern::{MatchBranch, Pattern};
use miri::ast::program::Program;
use miri::ast::statement::{ClassData, Statement, StatementKind};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;

fn span() -> Span {
    Span { start: 0, end: 0 }
}

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

fn int_expr() -> Box<miri::ast::Expression> {
    Box::new(identifier("int"))
}

fn assert_custom(t: &Type, name: &str) {
    match &t.kind {
        TypeKind::Custom(n, _) => assert_eq!(n, name),
        other => panic!("expected Custom({name}, ..), got {:?}", other),
    }
}

#[test]
fn test_normalize_list_to_custom() {
    let mut t = ty(TypeKind::List(int_expr()));
    normalize_type(&mut t);
    assert_custom(&t, "List");
}

#[test]
fn test_normalize_array_to_custom() {
    let mut t = ty(TypeKind::Array(
        int_expr(),
        Box::new(int_literal_expression(4)),
    ));
    normalize_type(&mut t);
    assert_custom(&t, "Array");
}

#[test]
fn test_normalize_map_to_custom() {
    let mut t = ty(TypeKind::Map(int_expr(), int_expr()));
    normalize_type(&mut t);
    assert_custom(&t, "Map");
}

#[test]
fn test_normalize_set_to_custom() {
    let mut t = ty(TypeKind::Set(int_expr()));
    normalize_type(&mut t);
    assert_custom(&t, "Set");
}

#[test]
fn test_normalize_preserves_primitives() {
    let mut t = ty(TypeKind::Int);
    normalize_type(&mut t);
    assert_eq!(t.kind, TypeKind::Int);
}

#[test]
fn test_normalize_option_recurses_into_inner() {
    let mut t = ty(TypeKind::Option(Box::new(ty(TypeKind::List(int_expr())))));
    normalize_type(&mut t);
    match &t.kind {
        TypeKind::Option(inner) => assert_custom(inner, "List"),
        other => panic!("expected Option(..), got {:?}", other),
    }
}

#[test]
fn test_normalize_linear_recurses_into_inner() {
    let mut t = ty(TypeKind::Linear(Box::new(ty(TypeKind::Set(int_expr())))));
    normalize_type(&mut t);
    match &t.kind {
        TypeKind::Linear(inner) => assert_custom(inner, "Set"),
        other => panic!("expected Linear(..), got {:?}", other),
    }
}

#[test]
fn test_normalize_custom_with_args_preserved() {
    let mut t = ty(TypeKind::Custom(
        "MyType".to_string(),
        Some(vec![identifier("T")]),
    ));
    normalize_type(&mut t);
    match &t.kind {
        TypeKind::Custom(n, Some(args)) => {
            assert_eq!(n, "MyType");
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected Custom(MyType, [T]), got {:?}", other),
    }
}

#[test]
fn test_normalize_void_unchanged() {
    let mut t = ty(TypeKind::Void);
    normalize_type(&mut t);
    assert_eq!(t.kind, TypeKind::Void);
}

#[test]
fn test_normalize_idempotent() {
    let mut t = ty(TypeKind::List(int_expr()));
    normalize_type(&mut t);
    let after_first = format!("{:?}", t.kind);
    normalize_type(&mut t);
    let after_second = format!("{:?}", t.kind);
    assert_eq!(after_first, after_second);
}

fn list_int_type_expr() -> Expression {
    type_expr_non_null(type_list(type_int()))
}

fn expect_type_expr_custom(expr: &Expression, expected_name: &str) {
    let ExpressionKind::Type(boxed, _) = &expr.node else {
        panic!("expected Type expression, got {:?}", expr.node);
    };
    match &boxed.kind {
        TypeKind::Custom(n, _) => assert_eq!(n, expected_name),
        other => panic!("expected Custom({expected_name}, ..), got {other:?}"),
    }
}

#[test]
fn test_normalize_walks_function_parameter_and_return_types() {
    let stmt = func("f")
        .params(vec![parameter(
            "xs".into(),
            list_int_type_expr(),
            None,
            None,
        )])
        .return_type(list_int_type_expr())
        .build_empty_body();
    let mut prog = Program { body: vec![stmt] };
    normalize(&mut prog);

    let StatementKind::FunctionDeclaration(decl) = &prog.body[0].node else {
        panic!("expected FunctionDeclaration");
    };
    expect_type_expr_custom(&decl.params[0].typ, "List");
    expect_type_expr_custom(decl.return_type.as_deref().expect("return type"), "List");
}

#[test]
fn test_normalize_walks_class_fields_and_methods() {
    let field = struct_member("xs", list_int_type_expr());
    let method = func("len")
        .return_type(type_expr_non_null(type_int()))
        .build_empty_body();
    let stmt = class_decl(
        "C",
        None,
        None,
        vec![],
        vec![
            Statement::new(0, StatementKind::Expression(field), Span::new(0, 0)),
            method,
        ],
        MemberVisibility::Public,
    );
    let mut prog = Program { body: vec![stmt] };
    normalize(&mut prog);

    let StatementKind::Class(data) = &prog.body[0].node else {
        panic!("expected Class");
    };
    let ClassData { body, .. } = data.as_ref();
    let StatementKind::Expression(field_expr) = &body[0].node else {
        panic!("expected Expression");
    };
    let ExpressionKind::StructMember(_, ty) = &field_expr.node else {
        panic!("expected StructMember");
    };
    expect_type_expr_custom(ty, "List");
}

#[test]
fn test_normalize_walks_struct_fields() {
    let stmt = struct_statement(
        identifier("S"),
        None,
        vec![struct_member("xs", list_int_type_expr())],
        vec![],
        MemberVisibility::Public,
    );
    let mut prog = Program { body: vec![stmt] };
    normalize(&mut prog);

    let StatementKind::Struct(_, _, fields, _, _) = &prog.body[0].node else {
        panic!("expected Struct");
    };
    let ExpressionKind::StructMember(_, ty) = &fields[0].node else {
        panic!("expected StructMember");
    };
    expect_type_expr_custom(ty, "List");
}

#[test]
fn test_normalize_walks_variable_declarations() {
    let var_decl = let_variable("a", Some(Box::new(list_int_type_expr())), None);
    let stmt = variable_statement(vec![var_decl], MemberVisibility::Public);
    let mut prog = Program { body: vec![stmt] };
    normalize(&mut prog);

    let StatementKind::Variable(decls, _) = &prog.body[0].node else {
        panic!("expected Variable");
    };
    expect_type_expr_custom(decls[0].typ.as_deref().expect("type"), "List");
}

#[test]
fn test_normalize_walks_for_loop_decls_and_iterable() {
    let body = block(vec![]);
    let decl = var("i", Some(Box::new(list_int_type_expr())), None);
    let stmt = for_statement(vec![decl], iter_obj(int_literal_expression(0)), body);
    let mut prog = Program { body: vec![stmt] };
    normalize(&mut prog);

    let StatementKind::For(decls, _, _) = &prog.body[0].node else {
        panic!("expected For");
    };
    expect_type_expr_custom(decls[0].typ.as_deref().expect("type"), "List");
}

#[test]
fn test_normalize_walks_return_expression_inside_function_body() {
    let body = return_statement(Some(Box::new(list(vec![int_literal_expression(1)]))));
    let stmt = func("f").build(body);
    let mut prog = Program { body: vec![stmt] };
    normalize(&mut prog);

    let StatementKind::FunctionDeclaration(decl) = &prog.body[0].node else {
        panic!("expected FunctionDeclaration");
    };
    let body = decl.body.as_deref().expect("body");
    let StatementKind::Return(Some(expr)) = &body.node else {
        panic!("expected Return(Some)");
    };
    assert!(matches!(expr.node, ExpressionKind::List(_)));
}

#[test]
fn test_normalize_walks_match_branch_bodies() {
    let branch = MatchBranch {
        patterns: vec![Pattern::Default],
        guard: None,
        body: Box::new(expression_statement(int_literal_expression(0))),
        is_mutable: false,
    };
    let var_decl = let_variable(
        "m",
        Some(Box::new(list_int_type_expr())),
        Some(Box::new(Expression::new(
            0,
            ExpressionKind::Match(Box::new(int_literal_expression(0)), vec![branch]),
            Span::new(0, 0),
        ))),
    );
    let stmt = variable_statement(vec![var_decl], MemberVisibility::Public);
    let mut prog = Program { body: vec![stmt] };
    normalize(&mut prog);
    let StatementKind::Variable(decls, _) = &prog.body[0].node else {
        panic!("expected Variable");
    };
    expect_type_expr_custom(decls[0].typ.as_deref().expect("type"), "List");
}

#[test]
fn test_normalize_walks_trait_member_types() {
    use miri::ast::factory::{struct_member, trait_decl};
    let stmt = trait_decl(
        "T",
        None,
        vec![],
        vec![Statement::new(
            0,
            StatementKind::Expression(struct_member("xs", list_int_type_expr())),
            Span::new(0, 0),
        )],
        MemberVisibility::Public,
    );
    let mut prog = Program { body: vec![stmt] };
    normalize(&mut prog);
    let StatementKind::Trait(_, _, _, body, _) = &prog.body[0].node else {
        panic!("expected Trait");
    };
    let StatementKind::Expression(field_expr) = &body[0].node else {
        panic!("expected Expression");
    };
    let ExpressionKind::StructMember(_, ty) = &field_expr.node else {
        panic!("expected StructMember");
    };
    expect_type_expr_custom(ty, "List");
}

#[test]
fn test_normalize_walks_while_body_variable_type() {
    use miri::ast::factory::{block, while_statement};
    let body = block(vec![miri::ast::factory::variable_statement(
        vec![let_variable(
            "xs",
            Some(Box::new(list_int_type_expr())),
            None,
        )],
        MemberVisibility::Public,
    )]);
    let stmt = while_statement(identifier("cond"), body);
    let mut prog = Program { body: vec![stmt] };
    normalize(&mut prog);
    let StatementKind::While(_, body, _) = &prog.body[0].node else {
        panic!("expected While");
    };
    let StatementKind::Block(stmts) = &body.node else {
        panic!("expected Block");
    };
    let StatementKind::Variable(decls, _) = &stmts[0].node else {
        panic!("expected Variable");
    };
    expect_type_expr_custom(decls[0].typ.as_deref().expect("type"), "List");
}

#[test]
fn test_normalize_walks_lambda_params_and_return_type() {
    use miri::ast::factory::{lambda, parameter};
    let lam = lambda()
        .params(vec![parameter(
            "xs".into(),
            list_int_type_expr(),
            None,
            None,
        )])
        .return_type(list_int_type_expr())
        .build_lambda_empty_body();
    let stmt = expression_statement(lam);
    let mut prog = Program { body: vec![stmt] };
    normalize(&mut prog);
    let StatementKind::Expression(expr) = &prog.body[0].node else {
        panic!("expected Expression");
    };
    let ExpressionKind::Lambda(data) = &expr.node else {
        panic!("expected Lambda");
    };
    expect_type_expr_custom(&data.params[0].typ, "List");
    expect_type_expr_custom(data.return_type.as_deref().expect("ret"), "List");
}

#[test]
fn test_normalize_walks_if_then_branch() {
    use miri::ast::factory::{block, if_statement};
    let then = block(vec![miri::ast::factory::variable_statement(
        vec![let_variable(
            "xs",
            Some(Box::new(list_int_type_expr())),
            None,
        )],
        MemberVisibility::Public,
    )]);
    let stmt = if_statement(identifier("cond"), then, None);
    let mut prog = Program { body: vec![stmt] };
    normalize(&mut prog);
    let StatementKind::If(_, then, _, _) = &prog.body[0].node else {
        panic!("expected If");
    };
    let StatementKind::Block(stmts) = &then.node else {
        panic!("expected Block");
    };
    let StatementKind::Variable(decls, _) = &stmts[0].node else {
        panic!("expected Variable");
    };
    expect_type_expr_custom(decls[0].typ.as_deref().expect("type"), "List");
}

#[test]
fn test_normalize_handles_nested_collection_in_set_inside_option() {
    let mut t = ty(TypeKind::Option(Box::new(ty(TypeKind::Set(Box::new(
        type_expr_non_null(type_list(type_string())),
    ))))));
    normalize_type(&mut t);
    let TypeKind::Option(inner) = &t.kind else {
        panic!("expected Option");
    };
    let TypeKind::Custom(set_name, Some(args)) = &inner.kind else {
        panic!("expected Custom Set");
    };
    assert_eq!(set_name, "Set");
    let ExpressionKind::Type(list_ty, _) = &args[0].node else {
        panic!("expected Type expression");
    };
    let TypeKind::Custom(list_name, _) = &list_ty.kind else {
        panic!("expected Custom List");
    };
    assert_eq!(list_name, "List");
}
