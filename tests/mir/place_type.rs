// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The declared type a place reaches, read projection by projection.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{Type, TypeKind, TUPLE_TYPE_NAME};
use miri::error::syntax::Span;
use miri::mir::place::PlaceElem;
use miri::mir::place_type::PlaceTypes;
use miri::mir::{Body, ExecutionModel, Local, LocalDecl, Place};

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn written(kind: TypeKind) -> Expression {
    Expression {
        id: 0,
        node: ExpressionKind::Type(Box::new(ty(kind)), false),
        span: Span::default(),
    }
}

/// Locals: 0 nothing, 1 `(String, int)`, 2 an index, 3 `[String]`,
/// 4 `(String, String)`, 5 the tuple class at `String`.
fn body() -> Body {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    for kind in [
        TypeKind::Void,
        TypeKind::Tuple(vec![written(TypeKind::String), written(TypeKind::Int)]),
        TypeKind::Int,
        TypeKind::List(Box::new(written(TypeKind::String))),
        TypeKind::Tuple(vec![written(TypeKind::String), written(TypeKind::String)]),
        TypeKind::Custom(
            TUPLE_TYPE_NAME.to_string(),
            Some(vec![written(TypeKind::String)]),
        ),
    ] {
        body.new_local(LocalDecl::new(ty(kind), Span::default()));
    }
    body
}

fn place(local: usize, projection: Vec<PlaceElem>) -> Place {
    Place {
        local: Local(local),
        projection,
    }
}

fn declared(body: &Body, place: &Place) -> Option<TypeKind> {
    PlaceTypes::of(body).declared(place).map(|ty| ty.kind)
}

#[test]
fn a_whole_local_reaches_its_declared_type() {
    let body = body();
    assert_eq!(declared(&body, &place(2, vec![])), Some(TypeKind::Int));
}

#[test]
fn a_tuple_field_reaches_its_element() {
    let body = body();
    let second = place(1, vec![PlaceElem::Field(1)]);
    assert_eq!(declared(&body, &second), Some(TypeKind::Int));
}

#[test]
fn a_collection_index_reaches_its_element() {
    let body = body();
    let element = place(3, vec![PlaceElem::Index(Local(2))]);
    assert_eq!(declared(&body, &element), Some(TypeKind::String));
}

#[test]
fn a_tuple_picked_by_a_value_reaches_its_element() {
    let body = body();
    let element = place(4, vec![PlaceElem::Index(Local(2))]);
    assert_eq!(declared(&body, &element), Some(TypeKind::String));
    let class_element = place(5, vec![PlaceElem::Index(Local(2))]);
    assert_eq!(declared(&body, &class_element), Some(TypeKind::String));
}

#[test]
fn nothing_is_read_behind_a_dereference() {
    let body = body();
    assert_eq!(declared(&body, &place(3, vec![PlaceElem::Deref])), None);
}
