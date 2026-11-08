// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A lowered body reads `Self` as the class it runs for, so no local it
//! declares, and no value it builds, still spells `Self`: as a trait's
//! parameter, as the bare keyword, or nested inside another type.
//!
//! `verify_resolved_self` is the only guard on an unresolved `Self` reaching
//! codegen: no later stage looks for one, and a `Self` local would be laid
//! out as an unknown type and skipped by reference counting. Every shape a
//! type can hide `Self` in is therefore covered here.

use miri::ast::bounds::Bounds;
use miri::ast::type_arguments::type_argument;
use miri::ast::types::{Type, TypeDeclarationKind, TypeKind};
use miri::error::syntax::Span;
use miri::mir::verify::verify_resolved_self;
use miri::mir::{
    AggregateKind, BasicBlockData, Body, ExecutionModel, LocalDecl, Place, Rvalue, Statement,
    StatementKind,
};

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::new(0, 0))
}

/// A trait's `Self`, bounded by the trait `Named`.
fn trait_self() -> TypeKind {
    TypeKind::Generic(
        "Self".to_string(),
        Bounds::one(
            TypeDeclarationKind::Implements,
            ty(TypeKind::Custom("Named".to_string(), None)),
        ),
    )
}

/// A body returning nothing and declaring one more local at `kind`.
fn body_declaring(kind: TypeKind) -> Body {
    let mut body = Body::new(0, Span::new(0, 0), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ty(TypeKind::Void), Span::new(0, 0)));
    body.new_local(LocalDecl::new(ty(kind), Span::new(0, 0)));
    body
}

#[test]
fn a_local_left_at_a_traits_self_is_reported() {
    let found = verify_resolved_self(&body_declaring(trait_self()));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].to_string().contains("still names `Self`"),
        "{}",
        found[0]
    );
}

#[test]
fn a_local_left_at_the_bare_keyword_is_reported() {
    let bare = TypeKind::Custom("Self".to_string(), None);
    assert_eq!(verify_resolved_self(&body_declaring(bare)).len(), 1);
}

#[test]
fn a_self_nested_in_a_type_argument_is_reported() {
    let nested = TypeKind::Custom(
        "Walk".to_string(),
        Some(vec![
            type_argument(ty(TypeKind::String)),
            type_argument(ty(trait_self())),
        ]),
    );
    assert_eq!(verify_resolved_self(&body_declaring(nested)).len(), 1);
}

#[test]
fn a_self_inside_an_optional_is_reported() {
    let optional = TypeKind::Option(Box::new(ty(trait_self())));
    assert_eq!(verify_resolved_self(&body_declaring(optional)).len(), 1);
}

#[test]
fn a_local_at_the_class_the_body_runs_for_is_not_reported() {
    let class = TypeKind::Custom("Words".to_string(), None);
    assert!(verify_resolved_self(&body_declaring(class)).is_empty());
}

#[test]
fn a_type_parameter_of_another_name_is_not_reported() {
    let parameter = TypeKind::Generic("T".to_string(), Bounds::none());
    assert!(verify_resolved_self(&body_declaring(parameter)).is_empty());
}

#[test]
fn an_instance_left_at_self_is_reported() {
    let mut body = body_declaring(TypeKind::Int);
    body.generics.instance = Some(ty(trait_self()));
    assert_eq!(verify_resolved_self(&body).len(), 1);
}

#[test]
fn a_class_value_built_at_a_type_spelling_self_is_reported() {
    let mut body = body_declaring(TypeKind::Int);
    let holder = ty(TypeKind::Custom(
        "Holder".to_string(),
        Some(vec![type_argument(ty(trait_self()))]),
    ));
    let mut block = BasicBlockData::new(None);
    block.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(miri::mir::Local(1)),
            Rvalue::Aggregate(AggregateKind::Class(holder), Vec::new()),
        ),
        span: Span::new(0, 0),
    });
    body.basic_blocks.push(block);
    let found = verify_resolved_self(&body);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].to_string().contains("builds a value at"));
}
