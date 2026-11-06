// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Discovery of the collection elements whose type has no declaration
//! to name.
//!
//! A collection releases the entries it discards through a drop callback — a
//! map's `key_drop_fn` and `val_drop_fn`, a list's or set's `elem_drop_fn` —
//! each of which needs the address of a decref function. An entry that is a
//! named type already has one (`miri.TypeName.$decref`); a tuple, an option or a
//! function value does not, because there is no declaration whose name could be
//! mangled into a symbol, so its thunk is named by the encoding of its
//! structure ([`structural_thunk_encoding`]). This module finds every
//! structural entry type a program uses so the matching thunk can be emitted
//! before any body that references it is compiled.

use crate::ast::type_arguments::{argument_type, argument_type_at, TypeArguments};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::mir::symbol::structural_thunk_encoding;
use crate::mir::Body;
use std::collections::{BTreeMap, HashSet};

/// A structural collection-entry type, the symbol suffix its decref thunk is
/// named by, and the parameters the bodies holding it leave open.
#[derive(Debug, Clone, PartialEq)]
pub struct StructuralElement {
    pub symbol: String,
    pub kind: TypeKind,
    /// Every parameter a body holding this entry type leaves open. An entry
    /// type written in a shared generic body (`(Pair<U>, int)`) still names its
    /// parameters, and its thunk releases them through the shared drop thunk,
    /// as the body itself would.
    pub open_params: HashSet<String>,
    /// Every parameter a body holding this entry type bound. A name bound in
    /// any holder is a substitution that holder missed, which the union of
    /// open parameters alone would hide behind another holder leaving the
    /// same name open.
    pub bound_params: HashSet<String>,
}

/// Every structural collection-entry type a program uses, deduplicated by
/// symbol suffix. Walks the declared type of every local in every body,
/// descending through composite types so a collection nested inside another
/// one is found too.
pub fn structural_element_types(bodies: &[(&str, &Body)]) -> Vec<StructuralElement> {
    let mut by_symbol: BTreeMap<String, StructuralElement> = BTreeMap::new();
    for (_, body) in bodies {
        let mut found: Vec<(String, TypeKind)> = Vec::new();
        for decl in &body.local_decls {
            collect(&decl.ty.kind, &mut found);
        }
        for (symbol, kind) in found {
            let element = by_symbol
                .entry(symbol.clone())
                .or_insert_with(|| StructuralElement {
                    symbol,
                    kind,
                    open_params: HashSet::new(),
                    bound_params: HashSet::new(),
                });
            element
                .open_params
                .extend(body.generics.open_params.iter().cloned());
            element
                .bound_params
                .extend(body.generics.bound_params.iter().cloned());
        }
    }
    by_symbol.into_values().collect()
}

/// Record the entry types of `kind` when it is a collection, then descend into
/// its component types looking for further collections.
fn collect(kind: &TypeKind, found: &mut Vec<(String, TypeKind)>) {
    match kind {
        TypeKind::Map(key, value) => {
            record(argument_type(key), found);
            record(argument_type(value), found);
            descend(argument_type(key), found);
            descend(argument_type(value), found);
        }
        TypeKind::Custom(name, Some(args)) => {
            record_builtin_entries(BuiltinCollectionKind::from_name(name), args, found);
            for arg in args {
                descend(argument_type(arg), found);
            }
        }
        TypeKind::Tuple(elements) => {
            for element in elements {
                descend(argument_type(element), found);
            }
        }
        TypeKind::List(elem) | TypeKind::Array(elem, _) | TypeKind::Set(elem) => {
            record(argument_type(elem), found);
            descend(argument_type(elem), found);
        }
        TypeKind::Result(ok, err) => {
            descend(argument_type(ok), found);
            descend(argument_type(err), found);
        }
        TypeKind::Future(inner) => descend(argument_type(inner), found),
        TypeKind::Option(inner) | TypeKind::Meta(inner) | TypeKind::Linear(inner) => {
            collect(&inner.kind, found)
        }
        TypeKind::Custom(_, None)
        | TypeKind::Generic(_, _, _)
        | TypeKind::OneOf(_)
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
    args: &TypeArguments,
    found: &mut Vec<(String, TypeKind)>,
) {
    match collection {
        Some(BuiltinCollectionKind::Map) => {
            record(argument_type_at(args, 0), found);
            record(argument_type_at(args, 1), found);
        }
        Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array)
        | Some(BuiltinCollectionKind::Set) => {
            record(argument_type_at(args, 0), found);
        }
        None => {}
    }
}

/// Record an entry type when it is structural, ignoring one that already has a
/// named thunk or no managed payload at all.
fn record(entry: Option<&Type>, found: &mut Vec<(String, TypeKind)>) {
    let Some(kind) = entry.map(|ty| &ty.kind) else {
        return;
    };
    if let Some(symbol) = structural_thunk_encoding(kind) {
        found.push((symbol, kind.clone()));
    }
}

/// Continue the walk through the type a type argument names.
fn descend(argument: Option<&Type>, found: &mut Vec<(String, TypeKind)>) {
    if let Some(ty) = argument {
        collect(&ty.kind, found);
    }
}
