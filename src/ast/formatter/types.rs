// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Renders type expressions back to Miri source syntax.
//!
//! [`TypeKind`]'s own `Display` is a diagnostic rendering: it spells a list as
//! `List(int)` so an error message reads well. Source syntax is different —
//! `[int]` — so types are rendered here rather than through `Display`.
//!
//! Some kinds never come from the parser. `Meta`, `Linear` and `Identifier`
//! are built by later phases and have no source spelling; they render as the
//! type they wrap, which is the closest faithful source form.

use crate::ast::expression::Expression;
use crate::ast::types::{FunctionTypeData, Type, TypeDeclarationKind, TypeKind};

use super::expression::expression as format_expression;
use super::helpers::parameter_list;
use super::sink::Sink;

/// Render a type, appending `?` when the type expression was written nullable.
pub fn type_expression(sink: &mut Sink, ty: &Type, is_nullable: bool) {
    type_kind(sink, &ty.kind);
    if is_nullable {
        sink.emit("?");
    }
}

/// Render a type kind in source syntax.
pub fn type_kind(sink: &mut Sink, kind: &TypeKind) {
    match kind {
        // Scalar spellings live with the type definitions, the one sanctioned
        // home for them; rendering them from there keeps a single spelling.
        TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::RawPtr
        | TypeKind::Void
        | TypeKind::Identifier
        | TypeKind::Error => sink.emit(&kind.to_string()),
        TypeKind::List(inner) => bracketed(sink, inner),
        TypeKind::Array(inner, size) => sized_array(sink, inner, size),
        TypeKind::Map(key, value) => map(sink, key, value),
        TypeKind::Set(inner) => braced(sink, inner),
        TypeKind::Tuple(members) => tuple(sink, members),
        TypeKind::Result(ok, err) => angled(sink, "Result", &[ok, err]),
        TypeKind::Future(inner) => angled(sink, "Future", &[inner]),
        TypeKind::Function(data) => function_type(sink, data),
        TypeKind::Generic(name, bound, declaration) => generic(sink, name, bound, *declaration),
        TypeKind::Custom(name, arguments) => custom(sink, name, arguments.as_deref()),
        TypeKind::Option(inner) => {
            type_kind(sink, &inner.kind);
            sink.emit("?");
        }
        // Built after parsing and never written in source: render what they wrap.
        TypeKind::Meta(inner) | TypeKind::Linear(inner) => type_kind(sink, &inner.kind),
    }
}

/// `[T]`
fn bracketed(sink: &mut Sink, inner: &Expression) {
    sink.emit("[");
    format_expression(sink, inner, 0);
    sink.emit("]");
}

/// `{T}`
fn braced(sink: &mut Sink, inner: &Expression) {
    sink.emit("{");
    format_expression(sink, inner, 0);
    sink.emit("}");
}

/// `[T; N]`
fn sized_array(sink: &mut Sink, inner: &Expression, size: &Expression) {
    sink.emit("[");
    format_expression(sink, inner, 0);
    sink.emit("; ");
    format_expression(sink, size, 0);
    sink.emit("]");
}

/// `{K: V}`
fn map(sink: &mut Sink, key: &Expression, value: &Expression) {
    sink.emit("{");
    format_expression(sink, key, 0);
    sink.emit(": ");
    format_expression(sink, value, 0);
    sink.emit("}");
}

/// `(A, B)`
fn tuple(sink: &mut Sink, members: &[Expression]) {
    sink.emit("(");
    for (index, member) in members.iter().enumerate() {
        if index > 0 {
            sink.emit(", ");
        }
        format_expression(sink, member, 0);
    }
    sink.emit(")");
}

/// `Name<A, B>`
fn angled(sink: &mut Sink, name: &str, arguments: &[&Expression]) {
    sink.emit(name);
    sink.emit("<");
    for (index, argument) in arguments.iter().enumerate() {
        if index > 0 {
            sink.emit(", ");
        }
        format_expression(sink, argument, 0);
    }
    sink.emit(">");
}

/// `fn(x int) float`
fn function_type(sink: &mut Sink, data: &FunctionTypeData) {
    sink.emit("fn");
    generic_arguments(sink, data.generics.as_deref());
    parameter_list(sink, &data.params);
    if let Some(return_type) = &data.return_type {
        sink.emit(" ");
        format_expression(sink, return_type, 0);
    }
}

/// `T`, `T extends Base`, `T is Base`
fn generic(
    sink: &mut Sink,
    name: &str,
    bound: &Option<Box<Type>>,
    declaration: TypeDeclarationKind,
) {
    sink.emit(name);
    let Some(bound) = bound else {
        return;
    };
    match declaration {
        TypeDeclarationKind::None => {}
        TypeDeclarationKind::Is
        | TypeDeclarationKind::Extends
        | TypeDeclarationKind::Implements
        | TypeDeclarationKind::Includes => {
            sink.emit(" ");
            sink.emit(&declaration.to_string());
            sink.emit(" ");
            type_kind(sink, &bound.kind);
        }
    }
}

/// `Name` or `Name<A, B>`
fn custom(sink: &mut Sink, name: &str, arguments: Option<&[Expression]>) {
    sink.emit(name);
    generic_arguments(sink, arguments);
}

/// `<A, B>`, or nothing when there are no arguments.
pub fn generic_arguments(sink: &mut Sink, arguments: Option<&[Expression]>) {
    let Some(arguments) = arguments else {
        return;
    };
    if arguments.is_empty() {
        return;
    }
    sink.emit("<");
    for (index, argument) in arguments.iter().enumerate() {
        if index > 0 {
            sink.emit(", ");
        }
        format_expression(sink, argument, 0);
    }
    sink.emit(">");
}
