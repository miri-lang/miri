// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Shared reference-counting helpers for MIR.
//!
//! This module is the single authority on whether a type requires RC management.
//! Both the Perceus optimization pass and the MIR lowering context delegate here.

use crate::ast::types::TypeKind;
use std::collections::HashSet;

/// Returns `true` if a type is managed (heap-allocated, needs RC).
///
/// Managed types are: Option, List, Array, Map, Set, Tuple, and Custom types
/// that are NOT in the auto-copy set and NOT generic placeholders.
pub fn is_managed_type(kind: &TypeKind, auto_copy_types: &HashSet<String>) -> bool {
    match kind {
        // Collections, Options, and Tuples use heap allocation and need RC.
        TypeKind::Option(_)
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Tuple(_) => true,
        // Note: String is excluded — it uses Box allocation, not alloc_with_rc,
        // so it doesn't have the [RC][payload] layout that IncRef/DecRef expect.
        TypeKind::Custom(name, _) => {
            // Exclude generic placeholders (Self, T, K, V, U) — they appear in
            // stdlib method bodies and represent unresolved types, not concrete
            // heap objects. Also exclude unresolved collection class names
            // (Array, List, Map, Set) that appear in stdlib method local_decls —
            // their locals may actually hold element values, not collections.
            // Auto-copy types use bitwise copy, no RC.
            !auto_copy_types.contains(name)
                && !matches!(
                    name.as_str(),
                    "Self" | "T" | "K" | "V" | "U" | "Array" | "List" | "Map" | "Set"
                )
        }
        _ => false,
    }
}
