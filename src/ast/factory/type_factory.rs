// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::expr;
use super::expression::generic_type_expression;
use super::literal::int_literal_expression;
use super::primitives::identifier;
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{
    BuiltinCollectionKind, FunctionTypeData, Type, TypeDeclarationKind, TypeKind, RESULT_TYPE_NAME,
};
use crate::ast::Parameter;
use crate::error::syntax::Span;

/// Creates a generic type expression bound to a name.
pub fn generic_type(name: &str, constraint: Option<Box<Expression>>) -> Expression {
    generic_type_expression(identifier(name), constraint, TypeDeclarationKind::None)
}

/// Creates a generic type with a specific declaration kind.
pub fn generic_type_with_kind(
    name: &str,
    constraint: Option<Box<Expression>>,
    kind: TypeDeclarationKind,
) -> Expression {
    generic_type_expression(identifier(name), constraint, kind)
}

/// Creates a type expression wrapping a Type.
pub fn type_expression(inner: Type, is_nullable: bool) -> Expression {
    expr(ExpressionKind::Type(Box::new(inner), is_nullable))
}

/// Creates a non-nullable type expression.
pub fn type_expr_non_null(t: Type) -> Expression {
    type_expression(t, false)
}

/// Creates an optional type expression.
pub fn type_expr_option(t: Type) -> Expression {
    type_expression(t, true)
}

/// Creates a Type of a specific kind with a default span.
pub fn make_type(kind: TypeKind) -> Type {
    Type::new(kind, Span::new(0, 0))
}

/// Creates an `Int` (arbitrary precision) type.
pub fn type_int() -> Type {
    make_type(TypeKind::Int)
}
/// Creates a `Float` (arbitrary precision) type.
pub fn type_float() -> Type {
    make_type(TypeKind::Float)
}
/// Creates a `String` type.
pub fn type_string() -> Type {
    make_type(TypeKind::String)
}
/// Creates a `Boolean` type.
pub fn type_bool() -> Type {
    make_type(TypeKind::Boolean)
}
/// Creates a `Void` type.
pub fn type_void() -> Type {
    make_type(TypeKind::Void)
}
/// Creates an `F64` type.
pub fn type_f64() -> Type {
    make_type(TypeKind::F64)
}
/// Creates an `F32` type.
pub fn type_f32() -> Type {
    make_type(TypeKind::F32)
}
/// Creates an `I128` type.
pub fn type_i128() -> Type {
    make_type(TypeKind::I128)
}
/// Creates an `I64` type.
pub fn type_i64() -> Type {
    make_type(TypeKind::I64)
}
/// Creates an `I32` type.
pub fn type_i32() -> Type {
    make_type(TypeKind::I32)
}
/// Creates an `I16` type.
pub fn type_i16() -> Type {
    make_type(TypeKind::I16)
}
/// Creates an `I8` type.
pub fn type_i8() -> Type {
    make_type(TypeKind::I8)
}
/// Creates a `U128` type.
pub fn type_u128() -> Type {
    make_type(TypeKind::U128)
}
/// Creates a `U64` type.
pub fn type_u64() -> Type {
    make_type(TypeKind::U64)
}
/// Creates a `U32` type.
pub fn type_u32() -> Type {
    make_type(TypeKind::U32)
}
/// Creates a `U16` type.
pub fn type_u16() -> Type {
    make_type(TypeKind::U16)
}
/// Creates a `U8` type.
pub fn type_u8() -> Type {
    make_type(TypeKind::U8)
}

/// Creates a `List` type.
pub fn type_list(inner: Type) -> Type {
    make_type(TypeKind::Custom(
        BuiltinCollectionKind::List.name().to_string(),
        Some(vec![type_expr_non_null(inner)]),
    ))
}

/// Creates an `Array` type.
pub fn type_array(inner: Type, size: i128) -> Type {
    make_type(TypeKind::Custom(
        BuiltinCollectionKind::Array.name().to_string(),
        Some(vec![
            type_expr_non_null(inner),
            int_literal_expression(size),
        ]),
    ))
}

/// Creates a `Map` type.
pub fn type_map(k: Type, v: Type) -> Type {
    make_type(TypeKind::Custom(
        BuiltinCollectionKind::Map.name().to_string(),
        Some(vec![type_expr_non_null(k), type_expr_non_null(v)]),
    ))
}

/// Creates a `Set` type.
pub fn type_set(inner: Type) -> Type {
    make_type(TypeKind::Custom(
        BuiltinCollectionKind::Set.name().to_string(),
        Some(vec![type_expr_non_null(inner)]),
    ))
}

/// Creates a `Tuple` type.
pub fn type_tuple(elements: Vec<Type>) -> Type {
    make_type(TypeKind::Tuple(
        elements.into_iter().map(type_expr_non_null).collect(),
    ))
}

/// Creates an optional wrapped type.
pub fn type_option(inner: Type) -> Type {
    make_type(TypeKind::Option(Box::new(inner)))
}

/// Creates a `Result` type.
///
/// Produces `Custom("Result", [ok, err])` — the canonical post-normalization
/// representation used by the type checker after `resolve_type_kind` converts
/// any `TypeKind::Result(...)` nodes.
pub fn type_result(ok: Type, err: Type) -> Type {
    make_type(TypeKind::Custom(
        RESULT_TYPE_NAME.to_string(),
        Some(vec![type_expr_non_null(ok), type_expr_non_null(err)]),
    ))
}

/// Creates a custom type (e.g., struct or class instance).
pub fn type_custom(name: &str, args: Option<Vec<Expression>>) -> Type {
    make_type(TypeKind::Custom(name.to_string(), args))
}

/// Creates a `Future` type.
pub fn type_future(inner: Type) -> Type {
    make_type(TypeKind::Future(Box::new(type_expr_non_null(inner))))
}

/// Creates a function signature type.
pub fn type_function(
    generics: Option<Vec<Expression>>,
    params: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
) -> Type {
    make_type(TypeKind::Function(Box::new(FunctionTypeData {
        generics,
        params,
        return_type,
    })))
}

/// Creates an `Identifier` type (internal, used for function/type references in MIR).
pub fn type_identifier() -> Type {
    make_type(TypeKind::Identifier)
}

/// Creates a `RawPtr` type (platform-width opaque pointer).
pub fn type_rawptr() -> Type {
    make_type(TypeKind::RawPtr)
}

/// Creates a type declaration expression (e.g., `T extends Number`).
pub fn type_declaration_expression(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    kind: TypeDeclarationKind,
    type_expr: Option<Box<Expression>>,
) -> Expression {
    expr(ExpressionKind::TypeDeclaration(
        Box::new(name),
        generic_types,
        kind,
        type_expr,
    ))
}

/// Creates a type declaration from a string name.
pub fn type_declaration(
    name: &str,
    generic_types: Option<Vec<Expression>>,
    kind: TypeDeclarationKind,
    type_expr: Option<Box<Expression>>,
) -> Expression {
    type_declaration_expression(identifier(name), generic_types, kind, type_expr)
}
