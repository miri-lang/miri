// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::common::{MemberVisibility, RuntimeKind};
use miri::ast::expression::{Expression, ExpressionKind, ImportPathKind, LeftHandSideExpression};
use miri::ast::factory::{
    array, assign, binary, boolean_literal, call, class_decl, class_identifier, const_variable,
    enum_value, expr_with_span, f_string, for_statement, func, identifier,
    identifier_literal_value, if_statement, import_path, import_path_multi, import_path_wildcard,
    int, int_literal_expression, iter_obj, let_variable, lhs_identifier, lhs_index, lhs_member,
    list, list_with_span, map, match_expression, member, named_argument, out_parameter, parameter,
    range, regex_literal, return_statement, runtime_function_declaration, set, stmt_with_span,
    string_literal_expression, struct_member, super_expression, tuple, type_array, type_bool,
    type_custom, type_expr_non_null, type_int, type_list, type_map, type_result, type_set,
    type_string, type_tuple, type_void, unless_statement, var, variable_statement, while_statement,
};
use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::operator::{AssignmentOp, BinaryOp};
use miri::ast::pattern::MatchBranch;
use miri::ast::statement::{
    IfStatementType, StatementKind, VariableDeclarationType, WhileStatementType,
};
use miri::ast::types::{BuiltinCollectionKind, TypeKind, RESULT_TYPE_NAME};
use miri::error::syntax::Span;

#[test]
fn int_picks_smallest_signed_width() {
    assert!(matches!(int(0), IntegerLiteral::I8(0)));
    assert!(matches!(int(127), IntegerLiteral::I8(127)));
    assert!(matches!(int(128), IntegerLiteral::I16(128)));
    assert!(matches!(int(32_768), IntegerLiteral::I32(32_768)));
    assert!(matches!(int(i32::MAX as i128 + 1), IntegerLiteral::I64(_)));
    assert!(matches!(int(i64::MAX as i128 + 1), IntegerLiteral::I128(_)));
    assert!(matches!(int(-129), IntegerLiteral::I16(-129)));
    assert!(matches!(int(i64::MIN as i128 - 1), IntegerLiteral::I128(_)));
}

#[test]
fn class_identifier_splits_on_double_colon() {
    let e = class_identifier("Http::Status");
    match e.node {
        ExpressionKind::Identifier(name, Some(class)) => {
            assert_eq!(name, "Status");
            assert_eq!(class, "Http");
        }
        other => panic!("expected qualified Identifier, got {other:?}"),
    }
}

#[test]
fn class_identifier_without_separator_returns_bare_identifier() {
    let e = class_identifier("BareName");
    match e.node {
        ExpressionKind::Identifier(name, class) => {
            assert_eq!(name, "BareName");
            assert!(class.is_none());
        }
        other => panic!("expected bare Identifier, got {other:?}"),
    }
}

#[test]
fn class_identifier_empty_returns_bare_empty_identifier() {
    let e = class_identifier("");
    match e.node {
        ExpressionKind::Identifier(name, class) => {
            assert!(name.is_empty());
            assert!(class.is_none());
        }
        other => panic!("expected bare Identifier, got {other:?}"),
    }
}

#[test]
fn function_builder_default_visibility_is_public() {
    let stmt = func("f").build_empty_body();
    let StatementKind::FunctionDeclaration(decl) = stmt.node else {
        panic!("expected FunctionDeclaration");
    };
    assert_eq!(decl.name, "f");
    assert_eq!(decl.properties.visibility, MemberVisibility::Public);
    assert!(!decl.properties.is_async);
    assert!(!decl.properties.is_parallel);
    assert!(!decl.properties.is_gpu);
    assert!(decl.body.is_some());
}

#[test]
fn function_builder_modifiers_compose() {
    let stmt = func("g")
        .set_async()
        .set_parallel()
        .set_gpu()
        .set_protected()
        .build_empty_body();
    let StatementKind::FunctionDeclaration(decl) = stmt.node else {
        panic!("expected FunctionDeclaration");
    };
    assert!(decl.properties.is_async);
    assert!(decl.properties.is_parallel);
    assert!(decl.properties.is_gpu);
    assert_eq!(decl.properties.visibility, MemberVisibility::Protected);
}

#[test]
fn function_builder_build_abstract_has_no_body() {
    let stmt = func("h").build_abstract();
    let StatementKind::FunctionDeclaration(decl) = stmt.node else {
        panic!("expected FunctionDeclaration");
    };
    assert!(decl.body.is_none());
}

#[test]
fn function_builder_build_lambda_produces_lambda_expression() {
    let e = func("").build_lambda_empty_body();
    assert!(matches!(e.node, ExpressionKind::Lambda(_)));
}

#[test]
fn parameter_helpers_set_is_out_flag() {
    let p = parameter("x".into(), type_expr_non_null(type_int()), None, None);
    assert!(!p.is_out);

    let p_out = out_parameter("y".into(), type_expr_non_null(type_int()), None, None);
    assert!(p_out.is_out);
}

#[test]
fn type_result_uses_centralized_name_constant() {
    let t = type_result(type_int(), type_string());
    match t.kind {
        TypeKind::Custom(name, Some(args)) => {
            assert_eq!(name, RESULT_TYPE_NAME);
            assert_eq!(args.len(), 2);
        }
        other => panic!("expected Custom(\"Result\", ..), got {other:?}"),
    }
}

#[test]
fn type_list_uses_builtin_collection_name() {
    let t = type_list(type_int());
    match t.kind {
        TypeKind::Custom(name, Some(args)) => {
            assert_eq!(name, BuiltinCollectionKind::List.name());
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected Custom(\"List\", ..), got {other:?}"),
    }
}

#[test]
fn type_array_carries_size_argument() {
    let t = type_array(type_int(), 4);
    match t.kind {
        TypeKind::Custom(name, Some(args)) => {
            assert_eq!(name, BuiltinCollectionKind::Array.name());
            assert_eq!(args.len(), 2);
        }
        other => panic!("expected Custom(\"Array\", ..), got {other:?}"),
    }
}

#[test]
fn type_map_and_set_use_builtin_collection_names() {
    assert!(matches!(
        type_map(type_int(), type_string()).kind,
        TypeKind::Custom(ref n, Some(ref args)) if n == BuiltinCollectionKind::Map.name() && args.len() == 2
    ));
    assert!(matches!(
        type_set(type_int()).kind,
        TypeKind::Custom(ref n, Some(ref args)) if n == BuiltinCollectionKind::Set.name() && args.len() == 1
    ));
}

#[test]
fn type_tuple_wraps_elements_in_non_null_type_expressions() {
    let t = type_tuple(vec![type_int(), type_string()]);
    let TypeKind::Tuple(elements) = t.kind else {
        panic!("expected Tuple");
    };
    assert_eq!(elements.len(), 2);
    for e in &elements {
        let ExpressionKind::Type(_, is_nullable) = e.node else {
            panic!("expected Type expression element");
        };
        assert!(!is_nullable);
    }
}

#[test]
fn type_custom_passes_through_name_and_args() {
    let t = type_custom("MyClass", Some(vec![identifier("T")]));
    match t.kind {
        TypeKind::Custom(n, Some(args)) => {
            assert_eq!(n, "MyClass");
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected Custom(MyClass, ..), got {other:?}"),
    }
}

#[test]
fn variable_declaration_helpers_set_declaration_type() {
    let v = let_variable("a", None, None);
    assert_eq!(v.declaration_type, VariableDeclarationType::Immutable);

    let m = var("b", None, None);
    assert_eq!(m.declaration_type, VariableDeclarationType::Mutable);

    let c = const_variable("c", None, None);
    assert_eq!(c.declaration_type, VariableDeclarationType::Constant);
}

#[test]
fn import_path_splits_segments_on_dot() {
    let e = import_path("foo.bar.baz");
    match e.node {
        ExpressionKind::ImportPath(segs, ImportPathKind::Simple) => {
            assert_eq!(segs.len(), 3);
            let names: Vec<_> = segs
                .iter()
                .map(|s| match &s.node {
                    ExpressionKind::Identifier(n, _) => n.as_str(),
                    _ => panic!("expected Identifier segments"),
                })
                .collect();
            assert_eq!(names, ["foo", "bar", "baz"]);
        }
        other => panic!("expected ImportPath(Simple), got {other:?}"),
    }
}

#[test]
fn import_path_wildcard_keeps_kind() {
    let e = import_path_wildcard("a.b");
    assert!(matches!(
        e.node,
        ExpressionKind::ImportPath(_, ImportPathKind::Wildcard)
    ));
}

#[test]
fn import_path_multi_carries_items() {
    let items = vec![(identifier("x"), None), (identifier("y"), None)];
    let e = import_path_multi("a.b", items);
    match e.node {
        ExpressionKind::ImportPath(_, ImportPathKind::Multi(items)) => {
            assert_eq!(items.len(), 2);
        }
        other => panic!("expected ImportPath(Multi), got {other:?}"),
    }
}

#[test]
fn if_and_unless_carry_distinct_kind() {
    let then = return_statement(None);
    let else_b = return_statement(None);
    let if_s = if_statement(boolean_literal(true), then.clone(), Some(else_b.clone()));
    match if_s.node {
        StatementKind::If(_, _, _, IfStatementType::If) => {}
        other => panic!("expected If(If), got {other:?}"),
    }
    let unless_s = unless_statement(boolean_literal(true), then, Some(else_b));
    match unless_s.node {
        StatementKind::If(_, _, _, IfStatementType::Unless) => {}
        other => panic!("expected If(Unless), got {other:?}"),
    }
}

#[test]
fn while_statement_uses_while_kind() {
    let body = return_statement(None);
    let w = while_statement(boolean_literal(true), body);
    match w.node {
        StatementKind::While(_, _, WhileStatementType::While) => {}
        other => panic!("expected While(While), got {other:?}"),
    }
}

#[test]
fn for_statement_carries_iterable_and_body() {
    let body = return_statement(None);
    let f = for_statement(
        vec![let_variable("i", None, None)],
        iter_obj(int_literal_expression(0)),
        body,
    );
    let StatementKind::For(decls, _, _) = f.node else {
        panic!("expected For");
    };
    assert_eq!(decls.len(), 1);
    assert_eq!(decls[0].name, "i");
}

#[test]
fn variable_statement_keeps_visibility() {
    let s = variable_statement(
        vec![let_variable("a", None, None)],
        MemberVisibility::Private,
    );
    let StatementKind::Variable(_, vis) = s.node else {
        panic!("expected Variable");
    };
    assert_eq!(vis, MemberVisibility::Private);
}

#[test]
fn runtime_function_declaration_carries_kind() {
    let s = runtime_function_declaration(RuntimeKind::Core, "miri_rt_x", vec![], None);
    match s.node {
        StatementKind::RuntimeFunctionDeclaration(kind, name, params, ret) => {
            assert_eq!(kind, RuntimeKind::Core);
            assert_eq!(name, "miri_rt_x");
            assert!(params.is_empty());
            assert!(ret.is_none());
        }
        other => panic!("expected RuntimeFunctionDeclaration, got {other:?}"),
    }
}

#[test]
fn class_decl_wraps_string_args_as_identifiers() {
    let s = class_decl(
        "Foo",
        None,
        Some("Base"),
        vec!["Trait1", "Trait2"],
        vec![],
        MemberVisibility::Public,
    );
    let StatementKind::Class(data) = s.node else {
        panic!("expected Class");
    };
    assert!(data.base_class.is_some());
    assert_eq!(data.traits.len(), 2);
    assert!(!data.is_abstract);
}

#[test]
fn lhs_helpers_match_their_variants() {
    let id = lhs_identifier("x");
    assert!(matches!(id, LeftHandSideExpression::Identifier(_)));

    let mem = lhs_member(identifier("a"), identifier("b"));
    assert!(matches!(mem, LeftHandSideExpression::Member(_)));

    let idx = lhs_index(identifier("a"), int_literal_expression(0));
    assert!(matches!(idx, LeftHandSideExpression::Index(_)));
}

#[test]
fn expression_helpers_produce_expected_variants() {
    let b = binary(
        int_literal_expression(1),
        BinaryOp::Add,
        int_literal_expression(2),
    );
    assert!(matches!(b.node, ExpressionKind::Binary(..)));

    let c = call(identifier("f"), vec![int_literal_expression(1)]);
    assert!(matches!(c.node, ExpressionKind::Call(..)));

    let m = member(identifier("o"), identifier("p"));
    assert!(matches!(m.node, ExpressionKind::Member(..)));

    let r = range(
        int_literal_expression(0),
        Some(Box::new(int_literal_expression(10))),
        miri::ast::expression::RangeExpressionType::Exclusive,
    );
    assert!(matches!(r.node, ExpressionKind::Range(..)));

    let a = assign(
        lhs_identifier("x"),
        AssignmentOp::Assign,
        int_literal_expression(1),
    );
    assert!(matches!(a.node, ExpressionKind::Assignment(..)));

    let l = list(vec![int_literal_expression(1)]);
    assert!(matches!(l.node, ExpressionKind::List(_)));

    let arr = array(
        vec![int_literal_expression(1)],
        Box::new(int_literal_expression(1)),
    );
    assert!(matches!(arr.node, ExpressionKind::Array(..)));

    let mp = map(vec![(identifier("k"), int_literal_expression(1))]);
    assert!(matches!(mp.node, ExpressionKind::Map(_)));

    let s = set(vec![int_literal_expression(1)]);
    assert!(matches!(s.node, ExpressionKind::Set(_)));

    let t = tuple(vec![
        int_literal_expression(1),
        string_literal_expression("a"),
    ]);
    assert!(matches!(t.node, ExpressionKind::Tuple(_)));

    let f = f_string(vec![string_literal_expression("a")]);
    assert!(matches!(f.node, ExpressionKind::FormattedString(_)));

    let na = named_argument("x".into(), int_literal_expression(1));
    assert!(matches!(na.node, ExpressionKind::NamedArgument(..)));

    let me = match_expression(int_literal_expression(0), Vec::<MatchBranch>::new());
    assert!(matches!(me.node, ExpressionKind::Match(..)));

    let sup = super_expression();
    assert!(matches!(sup.node, ExpressionKind::Super));
}

#[test]
fn enum_value_wraps_string_in_identifier() {
    let e = enum_value("Some", vec![int_literal_expression(1)]);
    let ExpressionKind::EnumValue(name, args) = e.node else {
        panic!("expected EnumValue");
    };
    assert!(matches!(name.node, ExpressionKind::Identifier(ref n, _) if n == "Some"));
    assert_eq!(args.len(), 1);
}

#[test]
fn struct_member_wraps_string_in_identifier() {
    let m = struct_member("field", type_expr_non_null(type_bool()));
    assert!(matches!(m.node, ExpressionKind::StructMember(..)));
}

#[test]
fn regex_literal_parses_flag_string() {
    let e = regex_literal("foo", "ig");
    let ExpressionKind::Literal(Literal::Regex(tok)) = e.node else {
        panic!("expected Regex literal");
    };
    assert_eq!(tok.body, "foo");
    assert!(tok.ignore_case);
    assert!(tok.global);
    assert!(!tok.multiline);
    assert!(!tok.dot_all);
    assert!(!tok.unicode);
}

#[test]
fn identifier_literal_value_constructs_identifier_variant() {
    let l = identifier_literal_value("foo");
    assert!(matches!(l, Literal::Identifier(ref s) if s == "foo"));
}

#[test]
fn expr_and_stmt_with_span_preserve_span() {
    let span = Span::new(10, 20);
    let e: Expression = expr_with_span(ExpressionKind::Super, span);
    assert_eq!(e.span, span);

    let s = stmt_with_span(StatementKind::Empty, span);
    assert_eq!(s.span, span);
}

#[test]
fn list_with_span_preserves_span() {
    let span = Span::new(5, 7);
    let e = list_with_span(vec![int_literal_expression(1)], span);
    assert_eq!(e.span, span);
}

#[test]
fn type_void_and_int_are_primitive_kinds() {
    assert_eq!(type_void().kind, TypeKind::Void);
    assert_eq!(type_int().kind, TypeKind::Int);
}

#[test]
fn with_span_helpers_preserve_span() {
    use miri::ast::expression::RangeExpressionType;
    use miri::ast::factory::{
        assign_with_span, binary_with_span, call_with_span, conditional_with_span,
        enum_value_expression_with_span, f_string_with_span, generic_type_expression_with_span,
        guard_with_span, identifier_with_class_and_span, identifier_with_span,
        import_path_expression_with_span, index_with_span, literal_with_span, logical_with_span,
        map_with_span, match_expression_with_span, member_with_span, named_argument_with_span,
        range_with_span, set_with_span, struct_member_expression_with_span,
        super_expression_with_span, tuple_with_span, type_declaration_expression_with_span,
        type_expression_with_span, type_int, unary_with_span,
    };
    use miri::ast::operator::{GuardOp, UnaryOp};
    use miri::ast::types::TypeDeclarationKind;

    let span = Span::new(11, 22);
    let int = || int_literal_expression(1);
    let id = || identifier("x");

    let checks: Vec<Expression> = vec![
        binary_with_span(int(), BinaryOp::Add, int(), span),
        unary_with_span(UnaryOp::Negate, int(), span),
        logical_with_span(
            boolean_literal(true),
            BinaryOp::And,
            boolean_literal(false),
            span,
        ),
        call_with_span(id(), vec![int()], span),
        member_with_span(id(), id(), span),
        index_with_span(id(), int(), span),
        assign_with_span(lhs_identifier("x"), AssignmentOp::Assign, int(), span),
        map_with_span(vec![(id(), int())], span),
        tuple_with_span(vec![int(), int()], span),
        set_with_span(vec![int()], span),
        match_expression_with_span(int(), Vec::<MatchBranch>::new(), span),
        f_string_with_span(vec![string_literal_expression("a")], span),
        type_expression_with_span(type_int(), false, span),
        generic_type_expression_with_span(id(), None, TypeDeclarationKind::None, span),
        conditional_with_span(
            int(),
            boolean_literal(true),
            None,
            IfStatementType::If,
            span,
        ),
        range_with_span(int(), None, RangeExpressionType::Exclusive, span),
        guard_with_span(GuardOp::Not, int(), span),
        import_path_expression_with_span(vec![id()], ImportPathKind::Simple, span),
        type_declaration_expression_with_span(id(), None, TypeDeclarationKind::None, None, span),
        enum_value_expression_with_span(id(), vec![int()], span),
        struct_member_expression_with_span(id(), id(), span),
        named_argument_with_span("x".into(), int(), span),
        identifier_with_span("x", span),
        identifier_with_class_and_span("x", Some("C".into()), span),
        literal_with_span(Literal::Boolean(true), span),
        super_expression_with_span(span),
    ];

    for (i, e) in checks.iter().enumerate() {
        assert_eq!(e.span, span, "helper #{i} dropped span");
    }
}
