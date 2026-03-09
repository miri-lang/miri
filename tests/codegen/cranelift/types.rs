// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use cranelift_codegen::ir::types;
use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{FunctionTypeData, Type, TypeDeclarationKind, TypeKind};
use miri::codegen::cranelift::translate_type;
use miri::error::syntax::Span;

fn t(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn e_ty(kind: TypeKind) -> Expression {
    Expression {
        id: 0,
        node: ExpressionKind::Type(Box::new(t(kind)), false),
        span: Span::default(),
    }
}

#[test]
fn test_translate_integer_types() {
    let ptr_ty = types::I64;

    // Signed integers
    assert_eq!(translate_type(&t(TypeKind::I8), ptr_ty), types::I8);
    assert_eq!(translate_type(&t(TypeKind::I16), ptr_ty), types::I16);
    assert_eq!(translate_type(&t(TypeKind::I32), ptr_ty), types::I32);
    assert_eq!(translate_type(&t(TypeKind::I64), ptr_ty), types::I64);
    assert_eq!(translate_type(&t(TypeKind::I128), ptr_ty), types::I128);

    // Unsigned integers
    assert_eq!(translate_type(&t(TypeKind::U8), ptr_ty), types::I8);
    assert_eq!(translate_type(&t(TypeKind::U16), ptr_ty), types::I16);
    assert_eq!(translate_type(&t(TypeKind::U32), ptr_ty), types::I32);
    assert_eq!(translate_type(&t(TypeKind::U64), ptr_ty), types::I64);
    assert_eq!(translate_type(&t(TypeKind::U128), ptr_ty), types::I128);

    // Platform-dependent integer
    assert_eq!(translate_type(&t(TypeKind::Int), ptr_ty), ptr_ty);
}

#[test]
fn test_translate_float_types() {
    let ptr_ty = types::I64;
    assert_eq!(translate_type(&t(TypeKind::F32), ptr_ty), types::F32);
    assert_eq!(translate_type(&t(TypeKind::F64), ptr_ty), types::F64);
    assert_eq!(translate_type(&t(TypeKind::Float), ptr_ty), types::F64);
}

#[test]
fn test_translate_primitive_types() {
    let ptr_ty = types::I64;
    assert_eq!(translate_type(&t(TypeKind::Boolean), ptr_ty), types::I8);
    assert_eq!(translate_type(&t(TypeKind::Void), ptr_ty), types::I8);
}

#[test]
fn test_translate_pointer_sized_types() {
    let ptr_ty = types::I64;
    assert_eq!(translate_type(&t(TypeKind::String), ptr_ty), ptr_ty);
    assert_eq!(translate_type(&t(TypeKind::Identifier), ptr_ty), ptr_ty);
    assert_eq!(translate_type(&t(TypeKind::RawPtr), ptr_ty), ptr_ty);
}

#[test]
fn test_translate_aggregate_and_collection_types() {
    let ptr_ty = types::I64;

    assert_eq!(
        translate_type(&t(TypeKind::List(Box::new(e_ty(TypeKind::I32)))), ptr_ty),
        ptr_ty
    );

    // Array size is an expression, we just use a dummy literal expression
    let array_size = Expression {
        id: 0,
        node: ExpressionKind::Literal(miri::ast::literal::Literal::Integer(
            miri::ast::literal::IntegerLiteral::I32(4),
        )),
        span: Span::default(),
    };
    assert_eq!(
        translate_type(
            &t(TypeKind::Array(
                Box::new(e_ty(TypeKind::I32)),
                Box::new(array_size.clone())
            )),
            ptr_ty
        ),
        ptr_ty
    );

    assert_eq!(
        translate_type(
            &t(TypeKind::Map(
                Box::new(e_ty(TypeKind::String)),
                Box::new(e_ty(TypeKind::I32))
            )),
            ptr_ty
        ),
        ptr_ty
    );
    assert_eq!(
        translate_type(&t(TypeKind::Set(Box::new(e_ty(TypeKind::I32)))), ptr_ty),
        ptr_ty
    );

    assert_eq!(
        translate_type(
            &t(TypeKind::Tuple(vec![
                e_ty(TypeKind::I32),
                e_ty(TypeKind::String)
            ])),
            ptr_ty
        ),
        ptr_ty
    );

    assert_eq!(
        translate_type(&t(TypeKind::Option(Box::new(t(TypeKind::I32)))), ptr_ty),
        ptr_ty
    );
    assert_eq!(
        translate_type(&t(TypeKind::Custom("MyStruct".to_string(), None)), ptr_ty),
        ptr_ty
    );
}

#[test]
fn test_translate_special_types() {
    let ptr_ty = types::I64;

    assert_eq!(
        translate_type(
            &t(TypeKind::Function(Box::new(FunctionTypeData {
                generics: None,
                params: vec![],
                return_type: Some(Box::new(e_ty(TypeKind::Void))),
            }))),
            ptr_ty
        ),
        ptr_ty
    );

    assert_eq!(
        translate_type(
            &t(TypeKind::Generic(
                "T".to_string(),
                None,
                TypeDeclarationKind::None
            )),
            ptr_ty
        ),
        ptr_ty
    );

    assert_eq!(
        translate_type(&t(TypeKind::Meta(Box::new(t(TypeKind::I32)))), ptr_ty),
        ptr_ty
    );
    assert_eq!(translate_type(&t(TypeKind::Error), ptr_ty), ptr_ty);
}
