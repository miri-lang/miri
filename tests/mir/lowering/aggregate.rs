// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{make_int_const, make_string_const};
use miri::ast::expression::ExpressionKind;
use miri::ast::statement::{Statement, StatementKind as AstStatementKind};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::lowering::lower_class_method;
use miri::mir::{AggregateKind, Body, Rvalue, StatementKind};
use miri::pipeline::Pipeline;

fn assert_display(rvalue: &Rvalue, expected: &str) {
    assert_eq!(format!("{}", rvalue), expected);
}

#[test]
fn test_aggregate_tuple_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Tuple,
        vec![make_int_const(1), make_int_const(2)],
    );
    assert_display(&rvalue, "(const Integer(I32(1)), const Integer(I32(2)))");
}

#[test]
fn test_aggregate_array_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Array,
        vec![make_int_const(1), make_int_const(2), make_int_const(3)],
    );
    assert_display(
        &rvalue,
        "[const Integer(I32(1)), const Integer(I32(2)), const Integer(I32(3))]",
    );
}

#[test]
fn test_aggregate_list_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::List,
        vec![make_int_const(1), make_int_const(2)],
    );
    assert_display(&rvalue, "[const Integer(I32(1)), const Integer(I32(2))]");
}

#[test]
fn test_aggregate_set_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Set,
        vec![make_int_const(1), make_int_const(2)],
    );
    assert_display(&rvalue, "{const Integer(I32(1)), const Integer(I32(2))}");
}

#[test]
fn test_aggregate_map_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Map,
        vec![
            make_string_const("a"),
            make_int_const(1),
            make_string_const("b"),
            make_int_const(2),
        ],
    );
    assert_display(
        &rvalue,
        "{const String(\"a\"): const Integer(I32(1)), const String(\"b\"): const Integer(I32(2))}",
    );
}

#[test]
fn test_aggregate_struct_display() {
    let ty = Type::new(TypeKind::Custom("Point".to_string(), None), Span::default());
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Struct(ty),
        vec![make_int_const(10), make_int_const(20)],
    );
    assert_display(
        &rvalue,
        "Point { const Integer(I32(10)), const Integer(I32(20)) }",
    );
}

#[test]
fn test_aggregate_equality() {
    let a = Rvalue::Aggregate(
        AggregateKind::Tuple,
        vec![make_int_const(1), make_int_const(2)],
    );
    let b = Rvalue::Aggregate(
        AggregateKind::Tuple,
        vec![make_int_const(1), make_int_const(2)],
    );
    assert_eq!(a, b);
}

#[test]
fn test_aggregate_cloning() {
    let original = Rvalue::Aggregate(
        AggregateKind::Array,
        vec![make_int_const(1), make_int_const(2)],
    );
    let cloned = original.clone();
    assert_eq!(original, cloned);
}

/// The method `method` of class `class`, as the parser produced it.
fn class_method<'a>(ast: &'a [Statement], class: &str, method: &str) -> Option<&'a Statement> {
    ast.iter()
        .filter_map(|stmt| {
            let AstStatementKind::Class(data) = &stmt.node else {
                return None;
            };
            Some(data)
        })
        .find(|data| matches!(&data.name.node, ExpressionKind::Identifier(name, _) if name == class))?
        .body
        .iter()
        .find(|member| {
            matches!(&member.node, AstStatementKind::FunctionDeclaration(func) if func.name == method)
        })
}

/// The type every `Some(..)` aggregate in `body` carries, in statement order.
fn optional_aggregate_types(body: &Body) -> Vec<TypeKind> {
    body.basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| {
            let (StatementKind::Assign(_, rvalue) | StatementKind::Reassign(_, rvalue)) =
                &statement.kind
            else {
                return None;
            };
            let Rvalue::Aggregate(AggregateKind::Option(ty), _) = rvalue else {
                return None;
            };
            Some(ty.kind.clone())
        })
        .collect()
}

#[test]
fn some_stored_into_a_class_field_carries_the_optional_type() {
    let source = r#"
class Node
    var value int

class Tree
    var left Node?

    fn attach(n Node)
        self.left = Some(n)

fn main()
    let t = Tree(left: None)
    t.attach(Node(value: 1))
"#;
    let result = Pipeline::new().frontend(source).expect("frontend failed");
    let method = class_method(&result.ast.body, "Tree", "attach").expect("Tree.attach not parsed");
    let tree = Type::new(TypeKind::Custom("Tree".to_string(), None), Span::default());
    let (body, _) =
        lower_class_method(method, tree, &result.type_checker, false).expect("lowering failed");
    let node = Type::new(TypeKind::Custom("Node".to_string(), None), Span::default());
    assert_eq!(
        optional_aggregate_types(&body),
        vec![TypeKind::Option(Box::new(node))],
        "`Some(n)` stored into `self.left` must carry `Node?`:\n{body}"
    );
}
