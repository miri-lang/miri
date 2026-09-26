// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::factory::type_expr_non_null;
use miri::ast::types::{Type, TypeKind};
use miri::codegen::cranelift::structural_elements::structural_thunk_symbol;
use miri::error::syntax::Span;

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

/// A pair whose second element is `kind`: a structural type, so it has an
/// encoding at all.
fn pair_with(kind: TypeKind) -> TypeKind {
    TypeKind::Tuple(vec![
        type_expr_non_null(ty(TypeKind::Int)),
        type_expr_non_null(ty(kind)),
    ])
}

fn encoding(kind: TypeKind) -> String {
    structural_thunk_symbol(&pair_with(kind)).expect("a tuple has an encoding")
}

fn declared(name: &str) -> TypeKind {
    TypeKind::Custom(name.to_string(), None)
}

/// A declared type named like a built-in kind's spelling drops as the
/// declared type, so the two never share a thunk.
#[test]
fn a_declared_type_named_like_a_built_in_kind_encodes_apart_from_it() {
    let pairs = [
        (declared("rawptr"), TypeKind::RawPtr),
        (declared("ident"), TypeKind::Identifier),
        (declared("error"), TypeKind::Error),
        (declared("void"), TypeKind::Void),
        (declared("int"), TypeKind::Int),
        (declared("String"), TypeKind::String),
    ];
    for (named, built_in) in pairs {
        assert_ne!(
            encoding(named.clone()),
            encoding(built_in.clone()),
            "{named:?} and {built_in:?} share an encoding"
        );
    }
}

/// A future, a meta type and a linear type around one payload drop
/// differently from each other, so each encodes under its own tag.
#[test]
fn wrappers_around_one_payload_encode_apart() {
    let inner = || Box::new(ty(TypeKind::String));
    let future = encoding(TypeKind::Future(Box::new(type_expr_non_null(ty(
        TypeKind::String,
    )))));
    let meta = encoding(TypeKind::Meta(inner()));
    let linear = encoding(TypeKind::Linear(inner()));
    assert_ne!(future, meta);
    assert_ne!(future, linear);
    assert_ne!(meta, linear);
}
