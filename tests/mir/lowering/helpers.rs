// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! When two spellings of a type are one value lowering may pass through
//! untouched, and when they need a coercion between them.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{Type, TypeDeclarationKind, TypeKind};
use miri::error::syntax::Span;
use miri::mir::lowering::helpers::spellings_of_one_value;

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn optional(kind: TypeKind) -> Type {
    ty(TypeKind::Option(Box::new(ty(kind))))
}

fn named(name: &str) -> TypeKind {
    TypeKind::Custom(name.to_string(), None)
}

fn generic(name: &str) -> TypeKind {
    TypeKind::Generic(name.to_string(), None, TypeDeclarationKind::None)
}

fn list_of(kind: TypeKind) -> TypeKind {
    let element = Expression::new(
        0,
        ExpressionKind::Type(Box::new(ty(kind)), false),
        Span::default(),
    );
    TypeKind::Custom("List".to_string(), Some(vec![element]))
}

#[test]
fn optional_of_a_parameter_and_of_a_class_are_not_one_value() {
    assert!(!spellings_of_one_value(
        &optional(named("T")),
        &optional(named("Node"))
    ));
    assert!(!spellings_of_one_value(
        &optional(generic("T")),
        &optional(named("Node"))
    ));
    assert!(!spellings_of_one_value(
        &optional(generic("T")),
        &optional(TypeKind::String)
    ));
    assert!(!spellings_of_one_value(
        &ty(TypeKind::Option(Box::new(optional(named("T"))))),
        &ty(TypeKind::Option(Box::new(optional(named("Node"))))),
    ));
}

#[test]
fn optionals_laid_out_alike_stay_one_value() {
    assert!(spellings_of_one_value(
        &optional(named("T")),
        &optional(TypeKind::Int)
    ));
    assert!(spellings_of_one_value(
        &optional(generic("T")),
        &optional(TypeKind::Int)
    ));
    assert!(spellings_of_one_value(
        &optional(list_of(generic("T"))),
        &optional(list_of(TypeKind::Int))
    ));
    assert!(spellings_of_one_value(
        &optional(TypeKind::Void),
        &optional(named("Node"))
    ));
}
