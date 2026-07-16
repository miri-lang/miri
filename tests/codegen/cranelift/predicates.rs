// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use miri::codegen::cranelift::translator::ElementShape;
use miri::codegen::cranelift::FunctionTranslator;
use miri::error::syntax::Span;

fn span() -> Span {
    Span::new(0, 0)
}

fn mk_type(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

fn mk_expr_type(kind: TypeKind) -> Expression {
    Expression::new(
        0,
        ExpressionKind::Type(Box::new(mk_type(kind)), false),
        span(),
    )
}

#[test]
fn classify_element_shape_scalar_types_are_other() {
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::I32),
        ElementShape::Other
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::F64),
        ElementShape::Other
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Boolean),
        ElementShape::Other
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Void),
        ElementShape::Other
    ));
}

#[test]
fn classify_element_shape_string() {
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::String),
        ElementShape::String
    ));
}

#[test]
fn classify_element_shape_builtin_collections() {
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::List(Box::new(mk_expr_type(
            TypeKind::I32
        )))),
        ElementShape::Builtin(BuiltinCollectionKind::List)
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Array(
            Box::new(mk_expr_type(TypeKind::I32)),
            Box::new(mk_expr_type(TypeKind::I64))
        )),
        ElementShape::Builtin(BuiltinCollectionKind::Array)
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Set(Box::new(mk_expr_type(
            TypeKind::I32
        )))),
        ElementShape::Builtin(BuiltinCollectionKind::Set)
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Map(
            Box::new(mk_expr_type(TypeKind::String)),
            Box::new(mk_expr_type(TypeKind::I32))
        )),
        ElementShape::Builtin(BuiltinCollectionKind::Map)
    ));
}

#[test]
fn classify_element_shape_vec_types_are_other() {
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Custom("Vec2".to_string(), None)),
        ElementShape::Other
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Custom("Vec3".to_string(), None)),
        ElementShape::Other
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Custom("Vec4".to_string(), None)),
        ElementShape::Other
    ));
}

#[test]
fn classify_element_shape_atomic_is_other() {
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Custom("Atomic".to_string(), None)),
        ElementShape::Other
    ));
}

#[test]
fn classify_element_shape_custom_builtin_normalized() {
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Custom(
            "List".to_string(),
            Some(vec![mk_expr_type(TypeKind::I32)])
        )),
        ElementShape::Builtin(BuiltinCollectionKind::List)
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Custom(
            "Array".to_string(),
            Some(vec![
                mk_expr_type(TypeKind::F32),
                mk_expr_type(TypeKind::I32)
            ])
        )),
        ElementShape::Builtin(BuiltinCollectionKind::Array)
    ));
}

#[test]
fn classify_element_shape_user_class() {
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Custom("MyClass".to_string(), None)),
        ElementShape::UserClass("MyClass")
    ));
}

#[test]
fn is_set_type_direct() {
    assert!(FunctionTranslator::is_set_type(&TypeKind::Set(Box::new(
        mk_expr_type(TypeKind::I32)
    ))));
}

#[test]
fn is_set_type_normalized_custom() {
    assert!(FunctionTranslator::is_set_type(&TypeKind::Custom(
        "Set".to_string(),
        Some(vec![mk_expr_type(TypeKind::I32)])
    )));
}

#[test]
fn is_set_type_not_other_collections() {
    assert!(!FunctionTranslator::is_set_type(&TypeKind::List(Box::new(
        mk_expr_type(TypeKind::I32)
    ))));
    assert!(!FunctionTranslator::is_set_type(&TypeKind::Array(
        Box::new(mk_expr_type(TypeKind::I32)),
        Box::new(mk_expr_type(TypeKind::I64))
    )));
    assert!(!FunctionTranslator::is_set_type(&TypeKind::Map(
        Box::new(mk_expr_type(TypeKind::String)),
        Box::new(mk_expr_type(TypeKind::I32))
    )));
}

#[test]
fn is_set_type_not_scalars() {
    assert!(!FunctionTranslator::is_set_type(&TypeKind::I32));
    assert!(!FunctionTranslator::is_set_type(&TypeKind::String));
    assert!(!FunctionTranslator::is_set_type(&TypeKind::Custom(
        "MyClass".to_string(),
        None
    )));
}

#[test]
fn is_list_type_direct() {
    assert!(FunctionTranslator::is_list_type(&TypeKind::List(Box::new(
        mk_expr_type(TypeKind::I32)
    ))));
}

#[test]
fn is_list_type_normalized() {
    assert!(FunctionTranslator::is_list_type(&TypeKind::Custom(
        "List".to_string(),
        Some(vec![mk_expr_type(TypeKind::I32)])
    )));
}

#[test]
fn is_list_type_not_array() {
    assert!(!FunctionTranslator::is_list_type(&TypeKind::Array(
        Box::new(mk_expr_type(TypeKind::I32)),
        Box::new(mk_expr_type(TypeKind::I64))
    )));
}

#[test]
fn is_map_type_direct() {
    assert!(FunctionTranslator::is_map_type(&TypeKind::Map(
        Box::new(mk_expr_type(TypeKind::String)),
        Box::new(mk_expr_type(TypeKind::I32))
    )));
}

#[test]
fn is_map_type_normalized() {
    assert!(FunctionTranslator::is_map_type(&TypeKind::Custom(
        "Map".to_string(),
        Some(vec![
            mk_expr_type(TypeKind::String),
            mk_expr_type(TypeKind::I32)
        ])
    )));
}

#[test]
fn is_collection_type_all_builtin_collections() {
    assert!(FunctionTranslator::is_collection_type(&TypeKind::List(
        Box::new(mk_expr_type(TypeKind::I32))
    )));
    assert!(FunctionTranslator::is_collection_type(&TypeKind::Array(
        Box::new(mk_expr_type(TypeKind::I32)),
        Box::new(mk_expr_type(TypeKind::I64))
    )));
    assert!(FunctionTranslator::is_collection_type(&TypeKind::Set(
        Box::new(mk_expr_type(TypeKind::I32))
    )));
    assert!(FunctionTranslator::is_collection_type(&TypeKind::Map(
        Box::new(mk_expr_type(TypeKind::String)),
        Box::new(mk_expr_type(TypeKind::I32))
    )));
}

#[test]
fn is_collection_type_not_scalars() {
    assert!(!FunctionTranslator::is_collection_type(&TypeKind::I32));
    assert!(!FunctionTranslator::is_collection_type(&TypeKind::String));
    assert!(!FunctionTranslator::is_collection_type(&TypeKind::Boolean));
}

#[test]
fn is_unsigned_type_kind_all_unsigned() {
    assert!(FunctionTranslator::is_unsigned_type_kind(&TypeKind::U8));
    assert!(FunctionTranslator::is_unsigned_type_kind(&TypeKind::U16));
    assert!(FunctionTranslator::is_unsigned_type_kind(&TypeKind::U32));
    assert!(FunctionTranslator::is_unsigned_type_kind(&TypeKind::U64));
    assert!(FunctionTranslator::is_unsigned_type_kind(&TypeKind::U128));
}

#[test]
fn is_unsigned_type_kind_not_signed() {
    assert!(!FunctionTranslator::is_unsigned_type_kind(&TypeKind::Int));
    assert!(!FunctionTranslator::is_unsigned_type_kind(&TypeKind::I8));
    assert!(!FunctionTranslator::is_unsigned_type_kind(&TypeKind::I32));
    assert!(!FunctionTranslator::is_unsigned_type_kind(&TypeKind::I64));
}

#[test]
fn is_integer_kind_signed() {
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::Int));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::I8));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::I16));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::I32));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::I64));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::I128));
}

#[test]
fn is_integer_kind_unsigned() {
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::U8));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::U16));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::U32));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::U64));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::U128));
}

#[test]
fn is_integer_kind_not_float() {
    assert!(!FunctionTranslator::is_integer_kind(&TypeKind::Float));
    assert!(!FunctionTranslator::is_integer_kind(&TypeKind::F32));
    assert!(!FunctionTranslator::is_integer_kind(&TypeKind::F64));
}

#[test]
fn is_integer_kind_not_other() {
    assert!(!FunctionTranslator::is_integer_kind(&TypeKind::String));
    assert!(!FunctionTranslator::is_integer_kind(&TypeKind::Boolean));
    assert!(!FunctionTranslator::is_integer_kind(&TypeKind::Void));
}
