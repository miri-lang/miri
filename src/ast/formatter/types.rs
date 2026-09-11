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
//!
//! A built-in collection has two spellings, its class name — `List<T>`,
//! `Map<K, V>`, `Set<T>`, `Array<T, N>`, `Tuple<A, B>` — and the sugar `[T]`,
//! `{K: V}`, `{T}`, `[T; N]`, `(A, B)`. The tree keeps neither, so a rendering
//! that can read the source writes back the one the author used, and a
//! rendering from the tree alone writes the sugar. The type checker names a
//! collection by its class; that rendering is written in the sugar too, so a
//! reader is shown one type one way whichever of the two it came from.

use crate::ast::expression::Expression;
use crate::ast::types::{
    BuiltinCollectionKind, FunctionTypeData, Type, TypeDeclarationKind, TypeKind, TUPLE_TYPE_NAME,
};
use crate::error::syntax::Span;

use super::expression::expression as format_expression;
use super::helpers::parameter_list;
use super::sink::Sink;

/// Render a type written at `span`, appending `?` when the type expression was
/// written nullable.
pub fn type_expression(sink: &mut Sink, ty: &Type, is_nullable: bool, span: Span) {
    match written_class_name(sink, &ty.kind, span) {
        Some((name, arguments)) => angled(sink, name, &arguments),
        None => type_kind(sink, &ty.kind),
    }
    if is_nullable {
        sink.emit("?");
    }
}

/// The class name and type arguments of a collection type the source at
/// `span` spells by its class name rather than in sugar.
///
/// The parser records a collection written by name at the name, and one
/// written in sugar at no position at all, so the source answers only for the
/// first. The name is compared as a whole word: the choice is between two
/// spellings of the same tree, never a reading of the type from the text.
fn written_class_name<'a>(
    sink: &Sink,
    kind: &'a TypeKind,
    span: Span,
) -> Option<(&'static str, Vec<&'a Expression>)> {
    let (name, arguments) = class_spelling(kind)?;
    let written = sink.source_text(span)?;
    let rest = written.strip_prefix(name)?;
    let continues_the_word = rest
        .chars()
        .next()
        .is_some_and(|next| next.is_alphanumeric() || next == '_');
    (!continues_the_word).then_some((name, arguments))
}

/// The class name a collection type can be written with, and the type
/// arguments that spelling carries.
fn class_spelling(kind: &TypeKind) -> Option<(&'static str, Vec<&Expression>)> {
    match kind {
        TypeKind::List(inner) => Some((BuiltinCollectionKind::List.name(), vec![inner])),
        TypeKind::Set(inner) => Some((BuiltinCollectionKind::Set.name(), vec![inner])),
        TypeKind::Map(key, value) => Some((BuiltinCollectionKind::Map.name(), vec![key, value])),
        TypeKind::Array(inner, size) => {
            Some((BuiltinCollectionKind::Array.name(), vec![inner, size]))
        }
        TypeKind::Tuple(members) if !members.is_empty() => {
            Some((TUPLE_TYPE_NAME, members.iter().collect()))
        }
        TypeKind::Tuple(_)
        | TypeKind::Int
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
        | TypeKind::Error
        | TypeKind::Result(..)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(..)
        | TypeKind::Custom(..)
        | TypeKind::Option(_)
        | TypeKind::Meta(_)
        | TypeKind::Linear(_) => None,
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
        TypeKind::Result(ok, err) => angled(sink, "Result", &[ok.as_ref(), err.as_ref()]),
        TypeKind::Future(inner) => angled(sink, "Future", &[inner.as_ref()]),
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

/// `Name` or `Name<A, B>`, or the sugar when `Name` is a built-in collection.
fn custom(sink: &mut Sink, name: &str, arguments: Option<&[Expression]>) {
    let collection = BuiltinCollectionKind::from_name(name);
    if let (Some(collection), Some(arguments)) = (collection, arguments) {
        if collection_sugar(sink, collection, arguments) {
            return;
        }
    }
    sink.emit(name);
    generic_arguments(sink, arguments);
}

/// Render a collection the type checker named by its class in the sugar, when
/// its arguments are the ones that sugar has room for. Answers whether it did.
fn collection_sugar(
    sink: &mut Sink,
    collection: BuiltinCollectionKind,
    arguments: &[Expression],
) -> bool {
    match (collection, arguments) {
        (BuiltinCollectionKind::List, [inner]) => bracketed(sink, inner),
        (BuiltinCollectionKind::Set, [inner]) => braced(sink, inner),
        (BuiltinCollectionKind::Map, [key, value]) => map(sink, key, value),
        (BuiltinCollectionKind::Array, [inner, size]) => sized_array(sink, inner, size),
        (
            BuiltinCollectionKind::List
            | BuiltinCollectionKind::Set
            | BuiltinCollectionKind::Map
            | BuiltinCollectionKind::Array,
            _,
        ) => return false,
    }
    true
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
