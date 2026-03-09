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

// ── Integer types ──────────────────────────────────────────────────────

#[test]
fn test_translate_signed_integers() {
    let ptr_ty = types::I64;
    assert_eq!(translate_type(&t(TypeKind::I8), ptr_ty), types::I8);
    assert_eq!(translate_type(&t(TypeKind::I16), ptr_ty), types::I16);
    assert_eq!(translate_type(&t(TypeKind::I32), ptr_ty), types::I32);
    assert_eq!(translate_type(&t(TypeKind::I64), ptr_ty), types::I64);
    assert_eq!(translate_type(&t(TypeKind::I128), ptr_ty), types::I128);
}

#[test]
fn test_translate_unsigned_integers() {
    let ptr_ty = types::I64;
    // Unsigned integers map to the same Cranelift types as signed (signedness is semantic, not IR-level)
    assert_eq!(translate_type(&t(TypeKind::U8), ptr_ty), types::I8);
    assert_eq!(translate_type(&t(TypeKind::U16), ptr_ty), types::I16);
    assert_eq!(translate_type(&t(TypeKind::U32), ptr_ty), types::I32);
    assert_eq!(translate_type(&t(TypeKind::U64), ptr_ty), types::I64);
    assert_eq!(translate_type(&t(TypeKind::U128), ptr_ty), types::I128);
}

#[test]
fn test_translate_platform_integer_follows_ptr_width() {
    // Int should follow the pointer width, not be hardcoded
    assert_eq!(translate_type(&t(TypeKind::Int), types::I64), types::I64);
    assert_eq!(translate_type(&t(TypeKind::Int), types::I32), types::I32);
}

// ── Float types ────────────────────────────────────────────────────────

#[test]
fn test_translate_float_types() {
    let ptr_ty = types::I64;
    assert_eq!(translate_type(&t(TypeKind::F32), ptr_ty), types::F32);
    assert_eq!(translate_type(&t(TypeKind::F64), ptr_ty), types::F64);
    // Platform-dependent float maps to F64
    assert_eq!(translate_type(&t(TypeKind::Float), ptr_ty), types::F64);
}

#[test]
fn test_float_is_independent_of_ptr_width() {
    // Float types should NOT change based on pointer width
    assert_eq!(translate_type(&t(TypeKind::F32), types::I32), types::F32);
    assert_eq!(translate_type(&t(TypeKind::F64), types::I32), types::F64);
    assert_eq!(translate_type(&t(TypeKind::Float), types::I32), types::F64);
}

// ── Primitive types ────────────────────────────────────────────────────

#[test]
fn test_translate_boolean() {
    let ptr_ty = types::I64;
    assert_eq!(translate_type(&t(TypeKind::Boolean), ptr_ty), types::I8);
}

#[test]
fn test_translate_void() {
    // Void is represented as I8 placeholder
    let ptr_ty = types::I64;
    assert_eq!(translate_type(&t(TypeKind::Void), ptr_ty), types::I8);
}

#[test]
fn test_boolean_and_void_independent_of_ptr_width() {
    assert_eq!(translate_type(&t(TypeKind::Boolean), types::I32), types::I8);
    assert_eq!(translate_type(&t(TypeKind::Void), types::I32), types::I8);
}

// ── Pointer-sized types ────────────────────────────────────────────────

#[test]
fn test_translate_pointer_sized_types() {
    let ptr_ty = types::I64;
    assert_eq!(translate_type(&t(TypeKind::String), ptr_ty), ptr_ty);
    assert_eq!(translate_type(&t(TypeKind::Identifier), ptr_ty), ptr_ty);
    assert_eq!(translate_type(&t(TypeKind::RawPtr), ptr_ty), ptr_ty);
}

#[test]
fn test_pointer_sized_types_follow_ptr_width() {
    // On a 32-bit target, pointer-sized types should be I32
    let ptr32 = types::I32;
    assert_eq!(translate_type(&t(TypeKind::String), ptr32), ptr32);
    assert_eq!(translate_type(&t(TypeKind::Identifier), ptr32), ptr32);
    assert_eq!(translate_type(&t(TypeKind::RawPtr), ptr32), ptr32);
}

// ── Collection types ───────────────────────────────────────────────────

#[test]
fn test_translate_list() {
    let ptr_ty = types::I64;
    assert_eq!(
        translate_type(&t(TypeKind::List(Box::new(e_ty(TypeKind::I32)))), ptr_ty),
        ptr_ty
    );
}

#[test]
fn test_translate_array() {
    let ptr_ty = types::I64;
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
                Box::new(array_size)
            )),
            ptr_ty
        ),
        ptr_ty
    );
}

#[test]
fn test_translate_map() {
    let ptr_ty = types::I64;
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
}

#[test]
fn test_translate_set() {
    let ptr_ty = types::I64;
    assert_eq!(
        translate_type(&t(TypeKind::Set(Box::new(e_ty(TypeKind::I32)))), ptr_ty),
        ptr_ty
    );
}

#[test]
fn test_translate_tuple() {
    let ptr_ty = types::I64;
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
}

#[test]
fn test_translate_empty_tuple() {
    let ptr_ty = types::I64;
    assert_eq!(translate_type(&t(TypeKind::Tuple(vec![])), ptr_ty), ptr_ty);
}

#[test]
fn test_translate_result() {
    let ptr_ty = types::I64;
    assert_eq!(
        translate_type(
            &t(TypeKind::Result(
                Box::new(e_ty(TypeKind::I32)),
                Box::new(e_ty(TypeKind::String))
            )),
            ptr_ty
        ),
        ptr_ty
    );
}

#[test]
fn test_translate_future() {
    let ptr_ty = types::I64;
    assert_eq!(
        translate_type(&t(TypeKind::Future(Box::new(e_ty(TypeKind::I32)))), ptr_ty),
        ptr_ty
    );
}

#[test]
fn test_translate_option() {
    let ptr_ty = types::I64;
    assert_eq!(
        translate_type(&t(TypeKind::Option(Box::new(t(TypeKind::I32)))), ptr_ty),
        ptr_ty
    );
}

// ── User-defined and special types ─────────────────────────────────────

#[test]
fn test_translate_custom_type() {
    let ptr_ty = types::I64;
    assert_eq!(
        translate_type(&t(TypeKind::Custom("MyStruct".to_string(), None)), ptr_ty),
        ptr_ty
    );
}

#[test]
fn test_translate_custom_type_with_generics() {
    let ptr_ty = types::I64;
    assert_eq!(
        translate_type(
            &t(TypeKind::Custom(
                "Vec".to_string(),
                Some(vec![e_ty(TypeKind::I32)])
            )),
            ptr_ty
        ),
        ptr_ty
    );
}

#[test]
fn test_translate_function_type() {
    let ptr_ty = types::I64;
    // Function with no params and void return
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

    // Function with no return type
    assert_eq!(
        translate_type(
            &t(TypeKind::Function(Box::new(FunctionTypeData {
                generics: None,
                params: vec![],
                return_type: None,
            }))),
            ptr_ty
        ),
        ptr_ty
    );
}

#[test]
fn test_translate_generic_type_variants() {
    let ptr_ty = types::I64;

    // Unbound generic
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

    // Generic with constraint
    assert_eq!(
        translate_type(
            &t(TypeKind::Generic(
                "T".to_string(),
                Some(Box::new(t(TypeKind::Custom(
                    "Comparable".to_string(),
                    None
                )))),
                TypeDeclarationKind::Extends
            )),
            ptr_ty
        ),
        ptr_ty
    );
}

#[test]
fn test_translate_meta_type() {
    let ptr_ty = types::I64;
    assert_eq!(
        translate_type(&t(TypeKind::Meta(Box::new(t(TypeKind::I32)))), ptr_ty),
        ptr_ty
    );
}

#[test]
fn test_translate_error_type() {
    let ptr_ty = types::I64;
    assert_eq!(translate_type(&t(TypeKind::Error), ptr_ty), ptr_ty);
}

#[test]
fn test_translate_linear_type() {
    let ptr_ty = types::I64;
    assert_eq!(
        translate_type(&t(TypeKind::Linear(Box::new(t(TypeKind::String)))), ptr_ty),
        ptr_ty
    );
    // Linear wrapping a primitive still maps to ptr (the Linear wrapper takes priority)
    assert_eq!(
        translate_type(&t(TypeKind::Linear(Box::new(t(TypeKind::I32)))), ptr_ty),
        ptr_ty
    );
}

// ── Cross-cutting: all collections are pointers on 32-bit ──────────────

#[test]
fn test_all_collections_are_pointer_sized_on_32bit() {
    let ptr32 = types::I32;

    assert_eq!(
        translate_type(&t(TypeKind::List(Box::new(e_ty(TypeKind::I32)))), ptr32),
        ptr32
    );
    assert_eq!(
        translate_type(&t(TypeKind::Set(Box::new(e_ty(TypeKind::I32)))), ptr32),
        ptr32
    );
    assert_eq!(
        translate_type(
            &t(TypeKind::Map(
                Box::new(e_ty(TypeKind::I32)),
                Box::new(e_ty(TypeKind::I32))
            )),
            ptr32
        ),
        ptr32
    );
    assert_eq!(
        translate_type(&t(TypeKind::Tuple(vec![e_ty(TypeKind::I32)])), ptr32),
        ptr32
    );
    assert_eq!(
        translate_type(
            &t(TypeKind::Result(
                Box::new(e_ty(TypeKind::I32)),
                Box::new(e_ty(TypeKind::String))
            )),
            ptr32
        ),
        ptr32
    );
    assert_eq!(
        translate_type(&t(TypeKind::Future(Box::new(e_ty(TypeKind::I32)))), ptr32),
        ptr32
    );
    assert_eq!(
        translate_type(&t(TypeKind::Option(Box::new(t(TypeKind::I32)))), ptr32),
        ptr32
    );
    assert_eq!(
        translate_type(&t(TypeKind::Custom("Foo".to_string(), None)), ptr32),
        ptr32
    );
}
