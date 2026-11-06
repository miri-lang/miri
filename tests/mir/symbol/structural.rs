// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::type_arguments::type_argument;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::symbol::{structural_thunk_encoding, Symbol, ThunkKind};

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

/// A pair whose second element is `kind`: a structural type, so it has an
/// encoding at all.
fn pair_with(kind: TypeKind) -> TypeKind {
    TypeKind::Tuple(vec![
        type_argument(ty(TypeKind::Int)),
        type_argument(ty(kind)),
    ])
}

fn encoding(kind: TypeKind) -> String {
    structural_thunk_encoding(&pair_with(kind)).expect("a tuple has an encoding")
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
    let future = encoding(TypeKind::Future(Box::new(type_argument(ty(
        TypeKind::String,
    )))));
    let meta = encoding(TypeKind::Meta(inner()));
    let linear = encoding(TypeKind::Linear(inner()));
    assert_ne!(future, meta);
    assert_ne!(future, linear);
    assert_ne!(meta, linear);
}

/// A structural type's thunk is the one its encoding names; a type with a
/// declaration or no managed payload has no structural thunk.
#[test]
fn a_structural_thunk_is_named_by_its_type_encoding() {
    let pair = pair_with(TypeKind::String);
    assert_eq!(
        Symbol::structural_thunk_of(ThunkKind::Decref, &pair),
        Some(Symbol::structural_thunk(
            ThunkKind::Decref,
            encoding(TypeKind::String)
        )),
    );
    assert_eq!(
        Symbol::structural_thunk_of(ThunkKind::Decref, &TypeKind::Int),
        None
    );
    assert_eq!(
        Symbol::structural_thunk_of(ThunkKind::Decref, &declared("Point")),
        None
    );
}

fn arg(kind: TypeKind) -> miri::ast::type_arguments::TypeArgument {
    type_argument(ty(kind))
}

fn tuple(kinds: Vec<TypeKind>) -> TypeKind {
    TypeKind::Tuple(kinds.into_iter().map(arg).collect())
}

/// A function value taking `params` and returning `ret`, or nothing.
fn closure(params: Vec<TypeKind>, ret: Option<TypeKind>) -> TypeKind {
    let params = params
        .into_iter()
        .enumerate()
        .map(|(index, kind)| miri::ast::common::Parameter {
            name: format!("p{index}"),
            name_span: Span::default(),
            typ: Box::new(miri::ast::factory::type_expr_non_null(ty(kind))),
            guard: None,
            default_value: None,
            is_out: false,
            residency: None,
        })
        .collect();
    TypeKind::Function(Box::new(miri::ast::types::FunctionTypeData {
        generics: None,
        params,
        return_type: ret.map(|kind| Box::new(miri::ast::factory::type_expr_non_null(ty(kind)))),
    }))
}

fn int_string() -> TypeKind {
    tuple(vec![TypeKind::Int, TypeKind::String])
}

fn array(element: TypeKind, size: i128) -> TypeKind {
    TypeKind::Array(
        Box::new(arg(element)),
        Box::new(miri::ast::type_arguments::value_argument(size)),
    )
}

fn sized_tag(size: i128) -> TypeKind {
    TypeKind::Custom(
        "Tag".to_string(),
        Some(vec![
            arg(TypeKind::Int),
            miri::ast::type_arguments::value_argument(size),
        ]),
    )
}

/// Every two structural types that drop, compare or hash differently encode
/// apart, so no two of them can share an `equals`, `hash` or decref thunk.
#[test]
fn structural_types_differing_anywhere_encode_apart() {
    let shapes = [
        tuple(vec![TypeKind::Int, int_string()]),
        tuple(vec![
            tuple(vec![TypeKind::Int, TypeKind::Int]),
            TypeKind::String,
        ]),
        tuple(vec![TypeKind::Int, TypeKind::String, TypeKind::Int]),
        int_string(),
        tuple(vec![TypeKind::String, TypeKind::Int]),
        pair_with(TypeKind::List(Box::new(arg(TypeKind::String)))),
        pair_with(TypeKind::Set(Box::new(arg(TypeKind::String)))),
        pair_with(array(TypeKind::String, 2)),
        pair_with(array(TypeKind::String, 3)),
        pair_with(sized_tag(2)),
        pair_with(sized_tag(3)),
        pair_with(TypeKind::Option(Box::new(ty(int_string())))),
        pair_with(TypeKind::Result(
            Box::new(arg(int_string())),
            Box::new(arg(TypeKind::String)),
        )),
        TypeKind::Option(Box::new(ty(int_string()))),
        pair_with(declared("t2.")),
        pair_with(declared("n1.a")),
        pair_with(declared("a")),
        pair_with(closure(vec![TypeKind::Int], Some(TypeKind::Int))),
        pair_with(closure(vec![TypeKind::String], Some(TypeKind::Int))),
        pair_with(closure(vec![TypeKind::Int], Some(TypeKind::String))),
        pair_with(closure(vec![TypeKind::Int], None)),
        pair_with(closure(
            vec![TypeKind::Int, TypeKind::Int],
            Some(TypeKind::Int),
        )),
        pair_with(TypeKind::List(Box::new(arg(closure(
            vec![TypeKind::Int],
            Some(TypeKind::Int),
        ))))),
        pair_with(TypeKind::List(Box::new(arg(closure(
            vec![TypeKind::String],
            Some(TypeKind::Int),
        ))))),
    ];
    for (i, left) in shapes.iter().enumerate() {
        for right in &shapes[i + 1..] {
            assert_ne!(
                structural_thunk_encoding(left),
                structural_thunk_encoding(right),
                "{left:?} and {right:?} share an encoding"
            );
        }
    }
}

/// A tuple whose bytes are its value is matched by those bytes and names no
/// equality or hash body; one holding a string, a float or padding between
/// its members names both, under its structural symbol.
#[test]
fn a_tuple_names_an_equality_only_when_its_bytes_are_not_its_value() {
    use miri::mir::dispatch::{element_hash_symbol, synthesized_equality_symbol};
    let definitions = std::collections::HashMap::new();
    let by_bytes = tuple(vec![TypeKind::Int, TypeKind::Int]);
    assert_eq!(synthesized_equality_symbol(&by_bytes, &definitions), None);
    assert_eq!(element_hash_symbol(&by_bytes, &definitions), None);
    for matched in [
        int_string(),
        tuple(vec![TypeKind::Float, TypeKind::Float]),
        tuple(vec![TypeKind::I8, TypeKind::Int]),
    ] {
        assert_eq!(
            synthesized_equality_symbol(&matched, &definitions),
            Symbol::structural_thunk_of(ThunkKind::Equals, &matched),
        );
        assert_eq!(
            element_hash_symbol(&matched, &definitions),
            Symbol::structural_thunk_of(ThunkKind::Hash, &matched),
        );
        assert!(Symbol::structural_thunk_of(ThunkKind::Hash, &matched).is_some());
    }
}
