// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Naming and discovery for collection elements whose type has no declaration
//! to name.
//!
//! A collection releases the entries it discards through a drop callback — a
//! map's `key_drop_fn` and `val_drop_fn`, a list's or set's `elem_drop_fn` —
//! each of which needs the address of a decref function. An entry that is a
//! named type already has one (`miri.TypeName.$decref`); a tuple, an option or a
//! function value does not, because there is no declaration whose name could be
//! mangled into a symbol. This module encodes such a type's structure into a
//! symbol suffix, and finds every structural entry type a program uses so the
//! matching thunk can be emitted before any body that references it is compiled.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{BuiltinCollectionKind, TypeKind};
use crate::mir::Body;

/// Symbol suffix naming the decref thunk for a structural type — a tuple, an
/// option or a function value, the managed shapes carrying no declared name.
/// `None` for every other kind, each of which already reaches a named thunk or
/// a per-shape runtime helper.
///
/// Every function value releases alike whatever its signature: the closure
/// carries its own capture destructor, so one thunk serves them all.
///
/// The encoding is a prefix code: `t<arity>.` introduces a tuple's elements,
/// `o` an option's payload, `c<count>.` a generic instantiation's arguments,
/// `u`, `k` and `z` the payload of a future, a meta type and a linear type,
/// `n<len>.` a leading-length declared name and `b<len>.` a built-in kind's
/// spelling — tagged apart from a declared name, since a program may declare
/// a type spelled like one. Every form is self-delimiting, so two types with
/// different drop behavior never encode alike and one thunk can never run
/// another type's field layout. A `.` cannot appear in a Miri
/// identifier, so the suffix also cannot collide with a user type's.
pub fn structural_thunk_symbol(kind: &TypeKind) -> Option<String> {
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
        | TypeKind::Meta(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_) => None,
    }
}

/// Append `kind`'s encoding to `out`. Composite kinds recurse through their
/// component types; a component the drop path ignores — an array's length, a
/// function's signature — is left out, so two types that drop identically share
/// one thunk.
fn encode(kind: &TypeKind, out: &mut String) {
    match kind {
        TypeKind::Tuple(elements) => {
            encode_arity('t', elements.len(), out);
            for element in elements {
                encode_expr(element, out);
            }
        }
        TypeKind::Option(inner) => {
            out.push('o');
            encode(&inner.kind, out);
        }
        TypeKind::List(elem) | TypeKind::Array(elem, _) | TypeKind::Set(elem) => {
            out.push('l');
            encode_expr(elem, out);
        }
        TypeKind::Map(key, value) => {
            out.push('m');
            encode_expr(key, out);
            encode_expr(value, out);
        }
        TypeKind::Result(ok, err) => {
            out.push('r');
            encode_expr(ok, out);
            encode_expr(err, out);
        }
        TypeKind::Custom(name, None) => encode_name(name, out),
        TypeKind::Custom(name, Some(args)) => {
            encode_arity('c', args.len(), out);
            encode_name(name, out);
            for arg in args {
                encode_expr(arg, out);
            }
        }
        TypeKind::Future(inner) => {
            out.push('u');
            encode_expr(inner, out);
        }
        TypeKind::Meta(inner) => {
            out.push('k');
            encode(&inner.kind, out);
        }
        TypeKind::Linear(inner) => {
            out.push('z');
            encode(&inner.kind, out);
        }
        TypeKind::Generic(name, _, _) => encode_name(name, out),
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
        TypeKind::String => encode_built_in(crate::ast::types::STRING_TYPE_NAME, out),
        TypeKind::Boolean => encode_built_in("bool", out),
        TypeKind::Identifier => encode_built_in("ident", out),
        TypeKind::RawPtr => encode_built_in("rawptr", out),
        TypeKind::Function(_) => encode_built_in("fn", out),
        TypeKind::Void => encode_built_in("void", out),
        TypeKind::Error => encode_built_in("error", out),
    }
}

/// Append a composite form's tag and component count, as in `t2.` for a pair.
fn encode_arity(tag: char, count: usize, out: &mut String) {
    out.push(tag);
    out.push_str(&count.to_string());
    out.push('.');
}

/// Encode the type carried by a type-argument expression. A value-generic
/// argument carries a literal rather than a type and contributes no drop
/// behavior, so it encodes as an empty name and keeps the surrounding form
/// self-delimiting.
fn encode_expr(expr: &Expression, out: &mut String) {
    match expr_kind(expr) {
        Some(kind) => encode(kind, out),
        None => encode_name("", out),
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

/// Every structural collection-entry type a program uses, paired with its
/// symbol suffix and deduplicated. Walks the declared type of every local in
/// every body, descending through composite types so a collection nested inside
/// another one is found too.
pub fn structural_element_types(bodies: &[(&str, &Body)]) -> Vec<(String, TypeKind)> {
    let mut found: Vec<(String, TypeKind)> = Vec::new();
    for (_, body) in bodies {
        for decl in &body.local_decls {
            collect(&decl.ty.kind, &mut found);
        }
    }
    found.sort_by(|(left, _), (right, _)| left.cmp(right));
    found.dedup_by(|(left, _), (right, _)| left == right);
    found
}

/// Record the entry types of `kind` when it is a collection, then descend into
/// its component types looking for further collections.
fn collect(kind: &TypeKind, found: &mut Vec<(String, TypeKind)>) {
    match kind {
        TypeKind::Map(key, value) => {
            record(expr_kind(key), found);
            record(expr_kind(value), found);
            descend(key, found);
            descend(value, found);
        }
        TypeKind::Custom(name, Some(args)) => {
            record_builtin_entries(BuiltinCollectionKind::from_name(name), args, found);
            for arg in args {
                descend(arg, found);
            }
        }
        TypeKind::Tuple(elements) => {
            for element in elements {
                descend(element, found);
            }
        }
        TypeKind::List(elem) | TypeKind::Array(elem, _) | TypeKind::Set(elem) => {
            record(expr_kind(elem), found);
            descend(elem, found);
        }
        TypeKind::Result(ok, err) => {
            descend(ok, found);
            descend(err, found);
        }
        TypeKind::Future(inner) => descend(inner, found),
        TypeKind::Option(inner) | TypeKind::Meta(inner) | TypeKind::Linear(inner) => {
            collect(&inner.kind, found)
        }
        TypeKind::Custom(_, None)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Function(_)
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
        | TypeKind::Void
        | TypeKind::Error => {}
    }
}

/// Record the entry types of a built-in collection written in its
/// post-normalization spelling: a map's key and value, or the element of a
/// list, array, or set. A non-collection name contributes nothing.
fn record_builtin_entries(
    collection: Option<BuiltinCollectionKind>,
    args: &[Expression],
    found: &mut Vec<(String, TypeKind)>,
) {
    match collection {
        Some(BuiltinCollectionKind::Map) => {
            record(args.first().and_then(expr_kind), found);
            record(args.get(1).and_then(expr_kind), found);
        }
        Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array)
        | Some(BuiltinCollectionKind::Set) => {
            record(args.first().and_then(expr_kind), found);
        }
        None => {}
    }
}

/// Record an entry type when it is structural, ignoring one that already has a
/// named thunk or no managed payload at all.
fn record(entry: Option<&TypeKind>, found: &mut Vec<(String, TypeKind)>) {
    let Some(kind) = entry else {
        return;
    };
    if let Some(symbol) = structural_thunk_symbol(kind) {
        found.push((symbol, kind.clone()));
    }
}

/// Continue the walk through a type-argument expression.
fn descend(expr: &Expression, found: &mut Vec<(String, TypeKind)>) {
    if let Some(kind) = expr_kind(expr) {
        collect(kind, found);
    }
}

/// The type an expression carries when it is a type argument, else `None`.
fn expr_kind(expr: &Expression) -> Option<&TypeKind> {
    let ExpressionKind::Type(ty, _) = &expr.node else {
        return None;
    };
    Some(&ty.kind)
}
