// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How a value of a type is represented at the top level, compared between a
//! body that leaves a type parameter open and an instantiation that binds it.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::representation::Representation;
use miri::type_checker::value_class::LayoutScope;
use std::collections::{HashMap, HashSet};

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

fn parameter() -> TypeKind {
    TypeKind::Custom("T".to_string(), None)
}

fn optional(kind: TypeKind) -> TypeKind {
    TypeKind::Option(Box::new(ty(kind)))
}

fn pair(first: TypeKind, second: TypeKind) -> TypeKind {
    TypeKind::Tuple(vec![written(first), written(second)])
}

fn list(element: TypeKind) -> TypeKind {
    TypeKind::List(Box::new(written(element)))
}

/// Whether `open`, read with `T` open, is represented as `bound` is in a
/// program, and the same with reference counting left out.
fn agreement(open: &TypeKind, bound: &TypeKind) -> (bool, bool) {
    let definitions = HashMap::new();
    let parameters = HashSet::from(["T".to_string()]);
    let shared = Representation::of(open, LayoutScope::new(&definitions, &parameters));
    let own = Representation::of(bound, LayoutScope::program(&definitions));
    let counted = shared == own;
    (counted, shared.without_counting() == own.without_counting())
}

#[test]
fn an_int_argument_is_represented_as_the_open_parameter() {
    assert_eq!(agreement(&parameter(), &TypeKind::Int), (true, true));
}

#[test]
fn a_float_or_a_narrow_argument_is_represented_apart_from_the_open_parameter() {
    assert_eq!(agreement(&parameter(), &TypeKind::Float), (false, false));
    assert_eq!(agreement(&parameter(), &TypeKind::I32), (false, false));
}

#[test]
fn a_counted_argument_differs_from_the_open_parameter_only_by_its_count() {
    assert_eq!(agreement(&parameter(), &TypeKind::String), (false, true));
}

/// A representation is the top level only: a list of `int` lists is a counted
/// pointer like a list of the open parameter, whatever is written inside.
#[test]
fn a_list_argument_is_compared_at_the_top_level_only() {
    assert_eq!(agreement(&parameter(), &list(TypeKind::Int)), (false, true));
    assert_eq!(
        agreement(&list(parameter()), &list(list(TypeKind::Int))),
        (true, true)
    );
}

#[test]
fn a_tuple_of_the_parameter_at_int_is_held_unlike_the_shared_tuple() {
    let open = pair(TypeKind::Int, parameter());
    assert_eq!(
        agreement(&open, &pair(TypeKind::Int, TypeKind::Int)),
        (false, false)
    );
    assert_eq!(
        agreement(&open, &pair(TypeKind::Int, TypeKind::String)),
        (true, true)
    );
}

/// The open parameter's optional is its word beside a tag, as `int?` is;
/// an optional of a reference is that reference instead, counted or not.
#[test]
fn an_optional_of_the_parameter_is_compared_layer_by_layer() {
    assert_eq!(
        agreement(&optional(parameter()), &optional(TypeKind::Int)),
        (true, true)
    );
    assert_eq!(
        agreement(&optional(parameter()), &optional(TypeKind::String)),
        (false, false)
    );
}
