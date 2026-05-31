// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::types::{BuiltinCollectionKind, TypeKind};
use miri::codegen::cranelift::translator::{
    is_capture_managed, is_field_managed, needs_out_pointer, ElementShape,
};
use miri::codegen::cranelift::FunctionTranslator;

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::Type;
use miri::error::syntax::Span;

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn expr_ty(kind: TypeKind) -> Expression {
    Expression {
        id: 0,
        node: ExpressionKind::Type(Box::new(ty(kind)), false),
        span: Span::default(),
    }
}

#[test]
fn needs_out_pointer_yes_for_scalars() {
    for k in [
        TypeKind::Int,
        TypeKind::I8,
        TypeKind::U8,
        TypeKind::I16,
        TypeKind::U16,
        TypeKind::I32,
        TypeKind::U32,
        TypeKind::I64,
        TypeKind::U64,
        TypeKind::I128,
        TypeKind::U128,
        TypeKind::F32,
        TypeKind::F64,
        TypeKind::Float,
        TypeKind::Boolean,
    ] {
        assert!(needs_out_pointer(&k), "{:?} should need an out pointer", k);
    }
}

#[test]
fn needs_out_pointer_no_for_managed() {
    assert!(!needs_out_pointer(&TypeKind::String));
    assert!(!needs_out_pointer(&TypeKind::List(Box::new(expr_ty(
        TypeKind::Int
    )))));
    assert!(!needs_out_pointer(&TypeKind::Custom(
        "MyClass".to_string(),
        None
    )));
    assert!(!needs_out_pointer(&TypeKind::Void));
}

#[test]
fn classify_element_shape_built_in_canonical() {
    let int_expr = Box::new(expr_ty(TypeKind::Int));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::String),
        ElementShape::String
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::List(int_expr.clone())),
        ElementShape::Builtin(BuiltinCollectionKind::List)
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Array(
            int_expr.clone(),
            Box::new(expr_ty(TypeKind::Int))
        )),
        ElementShape::Builtin(BuiltinCollectionKind::Array)
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Set(int_expr.clone())),
        ElementShape::Builtin(BuiltinCollectionKind::Set)
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Map(
            int_expr.clone(),
            int_expr.clone()
        )),
        ElementShape::Builtin(BuiltinCollectionKind::Map)
    ));
}

#[test]
fn classify_element_shape_custom_collapses_to_builtin() {
    let name = BuiltinCollectionKind::List.name().to_string();
    let list_custom = TypeKind::Custom(name, Some(vec![expr_ty(TypeKind::Int)]));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&list_custom),
        ElementShape::Builtin(BuiltinCollectionKind::List)
    ));
}

#[test]
fn classify_element_shape_custom_user_class() {
    let user = TypeKind::Custom("MyClass".to_string(), None);
    let shape = FunctionTranslator::classify_element_shape(&user);
    assert!(
        matches!(shape, ElementShape::UserClass("MyClass")),
        "expected UserClass(\"MyClass\"), got {:?}",
        shape
    );
}

#[test]
fn classify_element_shape_primitives_are_other() {
    for k in [
        TypeKind::Int,
        TypeKind::Boolean,
        TypeKind::F32,
        TypeKind::Void,
    ] {
        assert!(matches!(
            FunctionTranslator::classify_element_shape(&k),
            ElementShape::Other
        ));
    }
}

#[test]
fn is_field_managed_classifies_heap_types() {
    let int_expr = Box::new(expr_ty(TypeKind::Int));
    assert!(is_field_managed(&TypeKind::String));
    assert!(is_field_managed(&TypeKind::List(int_expr.clone())));
    assert!(is_field_managed(&TypeKind::Custom(
        "MyClass".to_string(),
        None
    )));
    assert!(!is_field_managed(&TypeKind::Int));
    assert!(!is_field_managed(&TypeKind::Boolean));
}

#[test]
fn is_capture_managed_includes_functions() {
    use miri::ast::types::FunctionTypeData;
    let fn_kind = TypeKind::Function(Box::new(FunctionTypeData {
        generics: None,
        params: Vec::new(),
        return_type: None,
    }));
    assert!(is_capture_managed(&fn_kind));
    // Reuses is_field_managed for everything else.
    assert!(is_capture_managed(&TypeKind::String));
    assert!(!is_capture_managed(&TypeKind::Int));
}

#[test]
fn is_unsigned_type_kind_only_unsigned_integers() {
    for k in [
        TypeKind::U8,
        TypeKind::U16,
        TypeKind::U32,
        TypeKind::U64,
        TypeKind::U128,
    ] {
        assert!(FunctionTranslator::is_unsigned_type_kind(&k));
    }
    for k in [
        TypeKind::I8,
        TypeKind::Int,
        TypeKind::F32,
        TypeKind::Boolean,
    ] {
        assert!(!FunctionTranslator::is_unsigned_type_kind(&k));
    }
}

#[test]
fn is_list_set_map_collection_type_predicates() {
    let int_expr = Box::new(expr_ty(TypeKind::Int));
    assert!(FunctionTranslator::is_list_type(&TypeKind::List(
        int_expr.clone()
    )));
    assert!(!FunctionTranslator::is_list_type(&TypeKind::Set(
        int_expr.clone()
    )));
    assert!(FunctionTranslator::is_set_type(&TypeKind::Set(
        int_expr.clone()
    )));
    assert!(FunctionTranslator::is_map_type(&TypeKind::Map(
        int_expr.clone(),
        int_expr.clone()
    )));
    assert!(FunctionTranslator::is_collection_type(&TypeKind::List(
        int_expr.clone()
    )));
    assert!(!FunctionTranslator::is_collection_type(&TypeKind::String));
}
