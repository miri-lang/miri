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
        // Collections, Options, Tuples, and Strings use heap allocation and need RC.
        TypeKind::Option(_) | TypeKind::Tuple(_) => true,
        // Canonical collection variants are normalized to Custom before RC analysis.
        // Keep them here as a safety net for any residual code paths.
        TypeKind::List(_) | TypeKind::Array(_, _) | TypeKind::Map(_, _) | TypeKind::Set(_) => true,
        // Strings are allocated via alloc_with_rc, freed via miri_rt_string_free.
        TypeKind::String => true,
        // Explicit generic type parameters are never concrete heap objects.
        TypeKind::Generic(_, _, _) => false,
        TypeKind::Custom(name, args) => {
            // Exclude generic placeholders that appear as Custom types (e.g. when
            // the type checker stores Custom("T", None) for a generic param reference).
            // Also exclude "Self" — a reserved keyword, never a user-defined type.
            // Auto-copy types use bitwise copy, no RC.
            if name == "Self"
                || auto_copy_types.contains(name.as_str())
                || type_params.contains(name.as_str())
            {
                return false;
            }

            // After normalization, builtin collection types arrive as
            // Custom("List", Some([...])) — these ARE heap-managed.
            // However, inside stdlib class bodies the names appear as
            // Custom("List", None) / Custom("Array", None) for unresolved generic
            // class self-references — those locals hold element values, not collections,
            // so they must NOT be treated as managed.
            if let Some(_kind) = BuiltinCollectionKind::from_name(name) {
                // If args is Some (instantiated), it's a real collection.
                // If args is None, it's the unresolved self-reference inside class body.
                return args.is_some();
            }

            // All other user-defined types (classes, structs, enums) are managed.
            true
        }
        _ => false,
    }
}
