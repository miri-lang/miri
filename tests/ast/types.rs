// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::factory::{identifier, int_literal_expression};
use miri::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use miri::error::syntax::Span;

fn span() -> Span {
    Span { start: 0, end: 0 }
}

fn boxed(expr: miri::ast::Expression) -> Box<miri::ast::Expression> {
    Box::new(expr)
}

#[test]
fn test_type_new() {
    let kind = TypeKind::Int;
    let s = Span { start: 10, end: 20 };
    let typ = Type::new(kind.clone(), s);
    assert_eq!(typ.kind, kind);
    assert_eq!(typ.span, s);
}

#[test]
fn test_builtin_collection_from_name_known() {
    assert_eq!(
        BuiltinCollectionKind::from_name("Array"),
        Some(BuiltinCollectionKind::Array)
    );
    assert_eq!(
        BuiltinCollectionKind::from_name("List"),
        Some(BuiltinCollectionKind::List)
    );
    assert_eq!(
        BuiltinCollectionKind::from_name("Map"),
        Some(BuiltinCollectionKind::Map)
    );
    assert_eq!(
        BuiltinCollectionKind::from_name("Set"),
        Some(BuiltinCollectionKind::Set)
    );
}

#[test]
fn test_builtin_collection_from_name_unknown() {
    assert_eq!(BuiltinCollectionKind::from_name(""), None);
    assert_eq!(BuiltinCollectionKind::from_name("array"), None);
    assert_eq!(BuiltinCollectionKind::from_name("String"), None);
    assert_eq!(BuiltinCollectionKind::from_name("Option"), None);
    assert_eq!(BuiltinCollectionKind::from_name("MyType"), None);
}

#[test]
fn test_builtin_collection_name_roundtrip() {
    for k in [
        BuiltinCollectionKind::Array,
        BuiltinCollectionKind::List,
        BuiltinCollectionKind::Map,
        BuiltinCollectionKind::Set,
    ] {
        assert_eq!(BuiltinCollectionKind::from_name(k.name()), Some(k));
    }
}

#[test]
fn test_as_builtin_collection_canonical_variants() {
    assert_eq!(
        TypeKind::List(boxed(identifier("T"))).as_builtin_collection(),
        Some(BuiltinCollectionKind::List)
    );
    assert_eq!(
        TypeKind::Array(boxed(identifier("T")), boxed(int_literal_expression(4)))
            .as_builtin_collection(),
        Some(BuiltinCollectionKind::Array)
    );
    assert_eq!(
        TypeKind::Map(boxed(identifier("K")), boxed(identifier("V"))).as_builtin_collection(),
        Some(BuiltinCollectionKind::Map)
    );
    assert_eq!(
        TypeKind::Set(boxed(identifier("T"))).as_builtin_collection(),
        Some(BuiltinCollectionKind::Set)
    );
}

#[test]
fn test_as_builtin_collection_custom() {
    assert_eq!(
        TypeKind::Custom("Array".to_string(), None).as_builtin_collection(),
        Some(BuiltinCollectionKind::Array)
    );
    assert_eq!(
        TypeKind::Custom("List".to_string(), None).as_builtin_collection(),
        Some(BuiltinCollectionKind::List)
    );
    assert_eq!(
        TypeKind::Custom("Map".to_string(), None).as_builtin_collection(),
        Some(BuiltinCollectionKind::Map)
    );
    assert_eq!(
        TypeKind::Custom("Set".to_string(), None).as_builtin_collection(),
        Some(BuiltinCollectionKind::Set)
    );
    assert_eq!(
        TypeKind::Custom("MyType".to_string(), None).as_builtin_collection(),
        None
    );
}

#[test]
fn test_as_builtin_collection_other_types() {
    assert_eq!(TypeKind::Int.as_builtin_collection(), None);
    assert_eq!(TypeKind::String.as_builtin_collection(), None);
    assert_eq!(TypeKind::Boolean.as_builtin_collection(), None);
    assert_eq!(TypeKind::Void.as_builtin_collection(), None);
    assert_eq!(
        TypeKind::Option(Box::new(Type::new(TypeKind::Int, span()))).as_builtin_collection(),
        None
    );
    assert_eq!(
        TypeKind::Tuple(vec![identifier("a"), identifier("b")]).as_builtin_collection(),
        None
    );
}

#[test]
fn test_is_copy_tuple_defaults_to_move() {
    let t = TypeKind::Tuple(vec![identifier("a"), identifier("b")]);
    assert!(
        !t.is_copy(),
        "Tuple defaults to Move; element types are not resolved at this layer"
    );
}

#[test]
fn test_is_copy_primitives() {
    for k in [
        TypeKind::Int,
        TypeKind::I8,
        TypeKind::I16,
        TypeKind::I32,
        TypeKind::I64,
        TypeKind::I128,
        TypeKind::U8,
        TypeKind::U16,
        TypeKind::U32,
        TypeKind::U64,
        TypeKind::U128,
        TypeKind::Float,
        TypeKind::F32,
        TypeKind::F64,
        TypeKind::Boolean,
        TypeKind::Identifier,
        TypeKind::RawPtr,
        TypeKind::Void,
        TypeKind::Error,
    ] {
        assert!(k.is_copy(), "expected {:?} to be Copy", k);
    }
}

#[test]
fn test_is_copy_complex_types_require_move() {
    let inner_int = || Box::new(identifier("int"));
    let pairs: Vec<TypeKind> = vec![
        TypeKind::String,
        TypeKind::List(inner_int()),
        TypeKind::Array(inner_int(), Box::new(int_literal_expression(4))),
        TypeKind::Map(inner_int(), inner_int()),
        TypeKind::Set(inner_int()),
        TypeKind::Result(inner_int(), inner_int()),
        TypeKind::Future(inner_int()),
        TypeKind::Custom("Foo".to_string(), None),
        TypeKind::Meta(Box::new(Type::new(TypeKind::Int, span()))),
    ];
    for k in pairs {
        assert!(!k.is_copy(), "expected {:?} to require Move", k);
    }
}

#[test]
fn test_is_copy_linear_never_copy() {
    let linear_int = TypeKind::Linear(Box::new(Type::new(TypeKind::Int, span())));
    assert!(!linear_int.is_copy());
}

#[test]
fn test_is_copy_option_inherits_inner() {
    let opt_int = TypeKind::Option(Box::new(Type::new(TypeKind::Int, span())));
    assert!(opt_int.is_copy());
    let opt_string = TypeKind::Option(Box::new(Type::new(TypeKind::String, span())));
    assert!(!opt_string.is_copy());
}
