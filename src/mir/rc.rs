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
/// that are NOT generic type parameters.
///
/// Auto-copy classification deliberately does not appear here. Whether a value
/// is copied bitwise on assignment is a question about assignment; whether its
/// storage must be released is a question about allocation. Every aggregate is
/// heap-allocated, so every aggregate needs releasing — an auto-copy struct that
/// was excluded here was allocated with nothing to free it.
///
/// `unmanaged_type_names` contains custom names that never denote a heap object
/// — type aliases resolving to an unmanaged type, which reach MIR still wearing
/// the alias name (`type Meters is int` arrives as `Custom("Meters")`).
///
/// `type_params` contains the names of in-scope generic type parameters
/// (e.g. `{"T", "K", "V"}` for a function `fn foo<T, K, V>(...)`).
/// A `Custom(name, _)` whose name is in `type_params` is an unresolved generic
/// placeholder — never a concrete heap object, so never managed.
pub fn is_managed_type(
    kind: &TypeKind,
    unmanaged_type_names: &HashSet<String>,
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
        // Closures/lambdas are heap-allocated via closure_alloc and reference-counted.
        TypeKind::Function(_) => true,
        // Explicit generic type parameters are never concrete heap objects.
        TypeKind::Generic(_, _, _) => false,
        TypeKind::Custom(name, args) => {
            // Exclude generic placeholders that appear as Custom types (e.g. when
            // the type checker stores Custom("T", None) for a generic param reference).
            // Also exclude "Self" — a reserved keyword, never a user-defined type.
            if name == "Self"
                || unmanaged_type_names.contains(name.as_str())
                || type_params.contains(name.as_str())
            {
                return false;
            }

            // Atomic<u32> and Atomic<i32> are scalar wrappers, not managed.
            if name == crate::ast::types::ATOMIC_TYPE_NAME {
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

/// Returns true if a field/element type is managed (heap-allocated, needs DecRef
/// on drop). Vector types (Vec2/3/4) are value types stored inline — when held
/// as a collection element or aggregate field they are raw bytes, not a managed
/// pointer, so they must never be DecRef'd here.
pub fn is_field_managed(kind: &TypeKind) -> bool {
    if let TypeKind::Custom(name, _) = kind {
        // Inline scalar/vector element wrappers (`Vec*`, `Atomic<scalar>`) are
        // stored by value, never reference-counted — exclude them from the
        // managed-element drop path. A vector is recognized as the field-layout
        // path recognizes one, so a user type that merely reuses the name stays
        // managed and its allocation is released.
        //
        // TODO: `Atomic` is still matched by name alone, so a user type of that
        // name is wrongly treated as an inline scalar and its allocation leaks.
        // Applying the same rule here needs `MirType::Custom` to carry the
        // component type too, or MIR keeps calling the user type unmanaged and
        // the two layers disagree about who releases it.
        //
        // TODO: a vector held as a field of a user struct reads back garbage and
        // leaks. A vector binding now carries its own allocation, so the struct's
        // slot holds a pointer while this predicate and the field-layout path
        // both read it as inline bytes — `s.v.x` decodes the pointer's low half
        // as a component. Deciding inline-versus-pointer by the field's position
        // rather than by its type alone is what closes it.
        if crate::ast::types::vec_type_dim(kind).is_some()
            || name == crate::ast::types::ATOMIC_TYPE_NAME
        {
            return false;
        }
    }
    matches!(
        kind,
        TypeKind::Option(_)
            | TypeKind::String
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Map(_, _)
            | TypeKind::Set(_)
            | TypeKind::Tuple(_)
            | TypeKind::Custom(_, _)
    )
}
