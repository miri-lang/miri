// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Shared reference-counting helpers for MIR.
//!
//! This module is the single authority on whether a type requires RC management.
//! Both the Perceus optimization pass and the MIR lowering context delegate here.

use crate::ast::types::{BuiltinCollectionKind, TypeKind};
use std::collections::HashSet;

/// Returns `true` if a type is managed (heap-allocated, needs RC).
///
/// Managed types are: Option, List, Array, Map, Set, Tuple, and Custom types
/// that are NOT in the auto-copy set and NOT generic type parameters.
///
/// `type_params` contains the names of in-scope generic type parameters
/// (e.g. `{"T", "K", "V"}` for a function `fn foo<T, K, V>(...)`).
/// A `Custom(name, _)` whose name is in `type_params` is an unresolved generic
/// placeholder — never a concrete heap object, so never managed.
pub fn is_managed_type(
    kind: &TypeKind,
    auto_copy_types: &HashSet<String>,
    type_params: &HashSet<String>,
) -> bool {
    match kind {
        // Collections, Options, and Tuples use heap allocation and need RC.
        TypeKind::Option(_)
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Tuple(_) => true,
        // Explicit generic type parameters are never concrete heap objects.
        TypeKind::Generic(_, _, _) => false,
        // Note: String is excluded — it uses Box allocation, not alloc_with_rc,
        // so it doesn't have the [RC][payload] layout that IncRef/DecRef expect.
        TypeKind::Custom(name, _) => {
            // Exclude generic placeholders that appear as Custom types (e.g. when
            // the type checker stores Custom("T", None) for a generic param reference).
            // Also exclude "Self" — a reserved keyword, never a user-defined type.
            // Also exclude unresolved collection class names (Array, List, Map, Set)
            // that appear in stdlib method local_decls — their locals may actually
            // hold element values rather than collections.
            // Auto-copy types use bitwise copy, no RC.
            name != "Self"
                && !auto_copy_types.contains(name.as_str())
                && !type_params.contains(name.as_str())
                && BuiltinCollectionKind::from_name(name).is_none()
        }
        _ => false,
    }
}
