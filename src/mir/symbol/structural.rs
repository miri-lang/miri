// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The encoding naming a structural type's thunks.
//!
//! A tuple, an option or a function value carries managed payload but has no
//! declaration whose name could be mangled into a symbol, so its decref,
//! `equals` and `hash` thunks are named by an encoding of its structure
//! instead, carried by [`Symbol::structural_thunk`]. The encoding is the one
//! naming authority for all three, so it keeps every part of the type any of
//! them depends on.

use super::{Symbol, ThunkKind};
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::BindingResidency;
use crate::ast::type_arguments::{ArithOp, TypeArgKind, TypeArgument, ValueArg};
use crate::ast::types::{FunctionTypeData, TypeKind, STRING_TYPE_NAME};

impl Symbol {
    /// The `kind` thunk of the structural type `ty`, or `None` when `ty` is
    /// not structural (see [`structural_thunk_encoding`]).
    pub fn structural_thunk_of(kind: ThunkKind, ty: &TypeKind) -> Option<Self> {
        structural_thunk_encoding(ty).map(|encoding| Self::structural_thunk(kind, encoding))
    }
}

/// Symbol suffix naming the thunks of a structural type — a tuple, an
/// option or a function value, the managed shapes carrying no declared name.
/// `None` for every other kind, each of which already reaches a named thunk or
/// a per-shape runtime helper.
///
/// The encoding is injective over everything a thunk's body depends on: how
/// the type is released, how `==` compares it and how it is hashed. Two types
/// encode alike only when every one of those is the same for both, so a
/// thunk can never run another type's layout or comparison. Nothing the type
/// is written with is left out, a function value's signature included, so
/// the encoding is injective on its own rather than through what the thunks
/// happen to read.
///
/// The encoding is a prefix code: `t<arity>.` introduces a tuple's elements,
/// `o` an option's payload (a nullable mark written on an argument reads as
/// one), `l`, `s` and `a` a list's, a set's and an array's element, an array
/// followed by its length, `m` a map's key and value, `r` a result's two
/// sides, `c<count>.` a generic instantiation's arguments, `u`, `k` and `z`
/// the payload of a future, a meta type and a linear type, `n<len>.` a
/// leading-length declared name, `p<len>.` a type parameter's name, `b<len>.`
/// a built-in kind's spelling — tagged apart from a declared name, since a
/// program may declare a type spelled like one — `f<arity>.` a function
/// value's signature (see [`encode_function`]) and `v` a value argument
/// (see [`encode_value`]). Every form is self-delimiting. A `.` cannot appear
/// in a Miri identifier, so the suffix also cannot collide with a user
/// type's.
pub fn structural_thunk_encoding(kind: &TypeKind) -> Option<String> {
    match kind {
        TypeKind::Tuple(_) | TypeKind::Option(_) | TypeKind::Function(_) => {
            let mut encoded = String::from(".");
            encode(kind, &mut encoded);
            Some(encoded)
        }
        TypeKind::Custom(_, _)
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
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::OneOf(_)
        | TypeKind::Meta(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_) => None,
    }
}

/// Append `kind`'s encoding to `out`. Composite kinds recurse through their
/// component types and values. A value argument carried as a marker type
/// encodes as the value it carries, as the symbol mangler reads it, so one
/// value is spelled one way whichever form holds it.
fn encode(kind: &TypeKind, out: &mut String) {
    if let Some(value) = crate::type_checker::generics::extract_value_generic_kind(kind) {
        return encode_argument(value, out);
    }
    match kind {
        TypeKind::Tuple(elements) => {
            encode_arity('t', elements.len(), out);
            elements
                .iter()
                .for_each(|element| encode_argument(element, out));
        }
        TypeKind::OneOf(members) => {
            encode_arity('o', members.len(), out);
            members
                .iter()
                .for_each(|member| encode_argument(member, out));
        }
        TypeKind::Option(inner) => {
            out.push('o');
            encode(&inner.kind, out);
        }
        TypeKind::List(elem) => encode_tagged('l', [elem.as_ref()], out),
        TypeKind::Set(elem) => encode_tagged('s', [elem.as_ref()], out),
        TypeKind::Array(elem, size) => encode_tagged('a', [elem.as_ref(), size.as_ref()], out),
        TypeKind::Map(key, value) => encode_tagged('m', [key.as_ref(), value.as_ref()], out),
        TypeKind::Result(ok, err) => encode_tagged('r', [ok.as_ref(), err.as_ref()], out),
        TypeKind::Custom(name, None) => encode_name(name, out),
        TypeKind::Custom(name, Some(args)) => {
            encode_arity('c', args.len(), out);
            encode_name(name, out);
            args.iter().for_each(|arg| encode_argument(arg, out));
        }
        TypeKind::Future(inner) => encode_tagged('u', [inner.as_ref()], out),
        TypeKind::Meta(inner) => {
            out.push('k');
            encode(&inner.kind, out);
        }
        TypeKind::Linear(inner) => {
            out.push('z');
            encode(&inner.kind, out);
        }
        TypeKind::Generic(name, _, _) => encode_leaf('p', name, out),
        TypeKind::Int => encode_built_in("int", out),
        TypeKind::I8 => encode_built_in("i8", out),
        TypeKind::I16 => encode_built_in("i16", out),
        TypeKind::I32 => encode_built_in("i32", out),
        TypeKind::I64 => encode_built_in("i64", out),
        TypeKind::I128 => encode_built_in("i128", out),
        TypeKind::U8 => encode_built_in("u8", out),
        TypeKind::U16 => encode_built_in("u16", out),
        TypeKind::U32 => encode_built_in("u32", out),
        TypeKind::U64 => encode_built_in("u64", out),
        TypeKind::U128 => encode_built_in("u128", out),
        TypeKind::Float => encode_built_in("float", out),
        TypeKind::F16 => encode_built_in("f16", out),
        TypeKind::F32 => encode_built_in("f32", out),
        TypeKind::F64 => encode_built_in("f64", out),
        TypeKind::String => encode_built_in(STRING_TYPE_NAME, out),
        TypeKind::Boolean => encode_built_in("bool", out),
        TypeKind::Identifier => encode_built_in("ident", out),
        TypeKind::RawPtr => encode_built_in("rawptr", out),
        TypeKind::Function(function) => encode_function(function, out),
        TypeKind::Void => encode_built_in("void", out),
        TypeKind::Error => encode_built_in("error", out),
    }
}

/// Append a component form's `tag` and the encoding of each of its fixed
/// number of `components`.
fn encode_tagged<const N: usize>(tag: char, components: [&TypeArgument; N], out: &mut String) {
    out.push(tag);
    components
        .into_iter()
        .for_each(|component| encode_argument(component, out));
}

/// Append a composite form's tag and component count, as in `t2.` for a pair.
fn encode_arity(tag: char, count: usize, out: &mut String) {
    out.push(tag);
    out.push_str(&count.to_string());
    out.push('.');
}

/// Encode a type argument: the type it names, read as an optional when it is
/// written with a nullable mark, or the value it holds.
fn encode_argument(argument: &TypeArgument, out: &mut String) {
    match &argument.node {
        TypeArgKind::Type(ty, marked) => {
            if *marked {
                out.push('o');
            }
            encode(&ty.kind, out);
        }
        TypeArgKind::Value(value) => {
            out.push('v');
            encode_value(value, out);
        }
    }
}

/// Append a function value's signature: `f<arity>.`, `g<count>.` and the
/// spelling of each type parameter it declares, then each parameter — `i` or
/// `w` for one taken by value or as an `out` parameter, `h` or `d` for one
/// bound on the host or the device, then its type — and last its return type,
/// `void` when it returns nothing.
fn encode_function(function: &FunctionTypeData, out: &mut String) {
    encode_arity('f', function.params.len(), out);
    let generics = function.generics.as_deref().unwrap_or_default();
    encode_arity('g', generics.len(), out);
    generics
        .iter()
        .for_each(|generic| encode_leaf('e', &generic.node.to_string(), out));
    for param in &function.params {
        out.push(if param.is_out { 'w' } else { 'i' });
        out.push(match param.residency {
            None | Some(BindingResidency::Host) => 'h',
            Some(BindingResidency::Gpu) => 'd',
        });
        encode_type_expression(&param.typ, out);
    }
    match function.return_type.as_deref() {
        Some(ret) => encode_type_expression(ret, out),
        None => encode_built_in("void", out),
    }
}

/// Append the type a type expression writes, read as an optional when it
/// carries a nullable mark; an expression naming no type directly is spelled
/// as it is written, under `e<len>.`.
fn encode_type_expression(expression: &Expression, out: &mut String) {
    match &expression.node {
        ExpressionKind::Type(ty, marked) => {
            if *marked {
                out.push('o');
            }
            encode(&ty.kind, out);
        }
        other => encode_leaf('e', &other.to_string(), out),
    }
}

/// Append a value argument: `i<len>.` and an integer's digits, `n<len>.` a
/// named value, `x`, the operator's letter and both operands for arithmetic,
/// and `g<len>.` the sign and magnitude of an integer past the 128-bit range.
fn encode_value(value: &ValueArg, out: &mut String) {
    match value {
        ValueArg::Int(number) => encode_leaf('i', &number.to_string(), out),
        ValueArg::Name(name) => encode_leaf('n', name, out),
        ValueArg::Binary(op, left, right) => {
            out.push('x');
            out.push(match op {
                ArithOp::Add => 'a',
                ArithOp::Sub => 's',
                ArithOp::Mul => 'm',
                ArithOp::Div => 'd',
                ArithOp::Mod => 'r',
            });
            encode_value(left, out);
            encode_value(right, out);
        }
        ValueArg::OutOfRange {
            negative,
            magnitude,
        } => {
            let sign = if *negative { "-" } else { "" };
            encode_leaf('g', &format!("{sign}{magnitude}"), out);
        }
    }
}

/// Append a length-prefixed declared name.
fn encode_name(name: &str, out: &mut String) {
    encode_leaf('n', name, out);
}

/// Append a length-prefixed built-in kind's spelling, tagged apart from a
/// declared name spelled the same.
fn encode_built_in(spelling: &str, out: &mut String) {
    encode_leaf('b', spelling, out);
}

/// Append `tag`, the length of `text`, `.` and `text`: a self-delimiting leaf.
fn encode_leaf(tag: char, text: &str, out: &mut String) {
    out.push(tag);
    out.push_str(&text.len().to_string());
    out.push('.');
    out.push_str(text);
}
