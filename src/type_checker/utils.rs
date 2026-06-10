// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Utility functions for the type checker.
//!
//! This module provides helper functions for:
//! - Type predicates (is_numeric, is_integer)
//! - Visibility checking
//! - Type expression manipulation
//! - Error reporting

use super::context::{Context, TypeDefinition};
use super::TypeChecker;
use crate::ast::factory::make_type;
use crate::ast::types::{
    BuiltinCollectionKind, Type, TypeKind, ACCELERABLE_TRAIT_NAME, DIM3_TYPE_NAME,
    GPU_CONTEXT_TYPE_NAME, KERNEL_TYPE_NAME,
};
use crate::ast::ExpressionKind;
use crate::ast::*;
use crate::error::format::find_best_match;
use crate::error::syntax::Span;
use crate::error::type_error::TypeError;

/// Determines whether a type is a resource — i.e., it defines `fn drop(self)` or
/// transitively contains a field whose type is a resource.
///
/// Resource types are subject to use-after-move tracking inside function bodies.
/// Managed types (String, List, collections, RC'd classes) are NOT resources.
///
/// # Generics
///
/// Generic type parameters are classified by their constraint:
/// - `T` (no bound) or `T extends ManagedClass` → not a resource (managed-typed
///   unknown; escape analysis applies).
/// - `T extends ResourceClass` (the bound class itself defines `fn drop` or
///   transitively contains a resource) → resource (strict-consume rule).
///
/// This makes the dispatch structural rather than nominal: every
/// monomorphization of a resource-bounded generic inherits the resource
/// classification from the bound, with no per-monomorphization re-analysis.
pub fn is_resource(
    kind: &TypeKind,
    type_definitions: &std::collections::HashMap<String, TypeDefinition>,
) -> bool {
    is_resource_inner(
        kind,
        type_definitions,
        &mut std::collections::HashSet::new(),
    )
}

fn is_resource_inner<'a>(
    kind: &'a TypeKind,
    type_definitions: &'a std::collections::HashMap<String, TypeDefinition>,
    visited: &mut std::collections::HashSet<&'a str>,
) -> bool {
    match kind {
        TypeKind::Custom(name, _) => {
            if !visited.insert(name.as_str()) {
                return false;
            }
            match type_definitions.get(name) {
                Some(TypeDefinition::Struct(def)) => {
                    if def.has_drop {
                        return true;
                    }
                    // Transitively check fields
                    def.fields
                        .iter()
                        .any(|(_, ty, _)| is_resource_inner(&ty.kind, type_definitions, visited))
                }
                Some(TypeDefinition::Class(def)) => {
                    if def.has_drop {
                        return true;
                    }
                    def.fields
                        .iter()
                        .any(|(_, fi)| is_resource_inner(&fi.ty.kind, type_definitions, visited))
                }
                _ => false,
            }
        }
        // A generic parameter is a resource iff its bound is a resource.
        // No bound (or non-resource bound) → managed-typed unknown.
        TypeKind::Generic(_, constraint, _) => constraint
            .as_ref()
            .is_some_and(|c| is_resource_inner(&c.kind, type_definitions, visited)),
        _ => false,
    }
}

/// Determines whether a type requires Perceus reference counting.
///
/// A type is Perceus-managed when it holds references to heap-allocated data
/// and cannot be bitwise-copied. This includes:
/// - Collections: `List<T>`, `Array<T>`, `Map<K,V>`, `Set<T>`
/// - Strings: heap-allocated via `alloc_with_rc`
/// - Options and Results over managed types
/// - Tuples containing managed types
/// - Closures/function values (heap-allocated)
/// - User-defined classes (which may hold managed fields)
/// - Structs with Perceus-managed fields
///
/// Pure scalar types (Int, Float, Bool, etc.) and generic parameters are NOT managed.
/// Auto-copy types (those in the `auto_copy_types` set) bypass RC.
pub fn is_perceus_managed(
    kind: &TypeKind,
    type_definitions: &std::collections::HashMap<String, TypeDefinition>,
) -> bool {
    is_perceus_managed_inner(
        kind,
        type_definitions,
        &mut std::collections::HashSet::new(),
    )
}

fn is_perceus_managed_inner(
    kind: &TypeKind,
    type_definitions: &std::collections::HashMap<String, TypeDefinition>,
    visited: &mut std::collections::HashSet<String>,
) -> bool {
    match kind {
        // Collections, Options, Tuples, and Strings use heap allocation and need RC.
        TypeKind::Option(elem_ty) => {
            is_perceus_managed_inner(&elem_ty.kind, type_definitions, visited)
        }
        TypeKind::Tuple(elems) => elems.iter().any(|elem_expr| {
            if let ExpressionKind::Type(elem, _) = &elem_expr.node {
                is_perceus_managed_inner(&elem.kind, type_definitions, visited)
            } else {
                false
            }
        }),
        TypeKind::List(elem_expr) => {
            if let ExpressionKind::Type(elem, _) = &elem_expr.node {
                is_perceus_managed_inner(&elem.kind, type_definitions, visited)
            } else {
                true // If we can't determine, assume managed
            }
        }
        TypeKind::Array(elem_expr, _) => {
            if let ExpressionKind::Type(elem, _) = &elem_expr.node {
                is_perceus_managed_inner(&elem.kind, type_definitions, visited)
            } else {
                true // If we can't determine, assume managed
            }
        }
        TypeKind::Map(_key_expr, _val_expr) => {
            // Maps are always managed regardless of key/value types
            true
        }
        TypeKind::Set(_elem_expr) => {
            // Sets are always managed
            true
        }
        TypeKind::String => true,
        TypeKind::Function(_) => true,
        TypeKind::Result(ok_expr, err_expr) => {
            let ok_managed = if let ExpressionKind::Type(t, _) = &ok_expr.node {
                is_perceus_managed_inner(&t.kind, type_definitions, visited)
            } else {
                false
            };
            let err_managed = if let ExpressionKind::Type(t, _) = &err_expr.node {
                is_perceus_managed_inner(&t.kind, type_definitions, visited)
            } else {
                false
            };
            ok_managed || err_managed
        }
        TypeKind::Generic(_, _, _) => false,
        TypeKind::Custom(name, args) => {
            // Exclude generic placeholders and "Self"
            if name == "Self" {
                return false;
            }

            // After normalization, builtin collection types are Custom("List", Some([...]))
            if BuiltinCollectionKind::from_name(name).is_some() {
                return args.is_some();
            }

            // Check user-defined types (classes and structs)
            if visited.contains(name) {
                return true; // Assume managed in cycles to be safe
            }
            visited.insert(name.clone());

            match type_definitions.get(name) {
                Some(TypeDefinition::Struct(def)) => def.fields.iter().any(|(_, field_ty, _)| {
                    is_perceus_managed_inner(&field_ty.kind, type_definitions, visited)
                }),
                Some(TypeDefinition::Class(_def)) => {
                    // All classes are managed (they have heap-allocated identity)
                    true
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// Determines whether a type is permitted inside a `gpu fn` body.
///
/// GPU kernels execute on the device with no heap allocator, no I/O, and no
/// string runtime — so only a strict subset of types may cross the call /
/// variable boundary in kernel code:
///
/// - Numeric primitives (all signed/unsigned integer widths, `Float`, `F32`,
///   `F64`) and `Boolean`.
/// - `Void` and `Error` (for soft-fail propagation of upstream errors).
/// - The compiler-builtin GPU types (`Dim3`, `GpuContext`, `Kernel`), the
///   builtin `Array<T>`, and the fixed-size `[T; N]` form, where the element
///   type `T` is itself GPU-compatible.
/// - `Generic` parameters — actual compatibility is enforced at the
///   instantiation site.
///
/// Everything else — `String`, heap collections (`List`, `Map`, `Set`),
/// `Tuple`, `Option`, `Result`, `Future`, function values, raw pointers,
/// user classes — is rejected. The check is structural and never
/// dispatches on stdlib names by string match: GPU builtins are looked up
/// via the canonical constants in [`crate::ast::types`].
pub fn is_gpu_compatible(kind: &TypeKind) -> bool {
    match kind {
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
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::Boolean
        | TypeKind::Void
        | TypeKind::Error => true,

        TypeKind::Generic(_, _, _) => true,

        TypeKind::Custom(name, type_args) => {
            if name == DIM3_TYPE_NAME || name == GPU_CONTEXT_TYPE_NAME || name == KERNEL_TYPE_NAME {
                return true;
            }
            if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Array) {
                return first_type_arg_is_gpu_compatible(type_args.as_deref());
            }
            false
        }

        TypeKind::Array(elem_expr, _size) => first_expr_type_is_gpu_compatible(elem_expr),

        TypeKind::String
        | TypeKind::List(_)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Tuple(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Option(_)
        | TypeKind::Linear(_)
        | TypeKind::Meta(_)
        | TypeKind::RawPtr
        | TypeKind::Identifier
        | TypeKind::Function(_) => false,
    }
}

/// Determines whether a type may back a gpu-resident binding (`gpu let` /
/// `gpu var`).
///
/// A type is *accelerable* when its bytes can be marshalled to device memory:
/// - an `AccelerableScalar` primitive — `int`, `i32`/`i64`, `u32`/`u64`,
///   `f32`/`f64`, `bool`;
/// - a nominal type whose definition implements the stdlib `Accelerable` trait,
///   provided every type argument is itself accelerable (this enforces the
///   `T : AccelerableScalar` element bound on the stdlib `Array` / `List` impls
///   without naming those types);
/// - a tuple whose every element type is accelerable.
///
/// Dispatch is by the `Accelerable` trait, never by stdlib type name. This
/// means new GPU-eligible containers (e.g. `Tensor<T, Rank>`) can be added
/// without modifying the compiler — only their `.mi` implementation is needed.
pub fn is_accelerable(
    kind: &TypeKind,
    type_definitions: &std::collections::HashMap<String, TypeDefinition>,
) -> bool {
    accelerable_inner(kind, type_definitions, false)
}

/// True when a type does not *preclude* accelerability: it is accelerable, or a
/// generic parameter whose concrete accelerability is enforced where the generic
/// is instantiated.
///
/// Used to validate an `implements Accelerable` declaration, where field types
/// may legitimately be generic parameters (`class Box<T> implements Accelerable`
/// with a `T` field is sound — `Box<String>` is rejected at the use site, not
/// here). The residency gate uses [`is_accelerable`] instead, which requires a
/// concrete accelerable type.
pub fn permits_accelerable(
    kind: &TypeKind,
    type_definitions: &std::collections::HashMap<String, TypeDefinition>,
) -> bool {
    accelerable_inner(kind, type_definitions, true)
}

/// Shared core of [`is_accelerable`] and [`permits_accelerable`]. `allow_generic`
/// decides whether an unresolved generic parameter counts as accelerable: `false`
/// at a residency binding (the type must be concrete), `true` when validating an
/// impl declaration (the generic is resolved at the instantiation site).
fn accelerable_inner(
    kind: &TypeKind,
    type_definitions: &std::collections::HashMap<String, TypeDefinition>,
    allow_generic: bool,
) -> bool {
    match kind {
        TypeKind::Int
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::Boolean => true,

        TypeKind::Generic(_, _, _) => allow_generic,

        TypeKind::Tuple(elements) => elements
            .iter()
            .all(|elem| expr_type_is_accelerable(elem, type_definitions, allow_generic)),

        TypeKind::Custom(name, args) => {
            type_implements_accelerable(name, type_definitions)
                && type_args_are_accelerable(args.as_deref(), type_definitions, allow_generic)
        }

        TypeKind::I8
        | TypeKind::I16
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::String
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Option(_)
        | TypeKind::Linear(_)
        | TypeKind::Meta(_)
        | TypeKind::RawPtr
        | TypeKind::Identifier
        | TypeKind::Function(_) => false,
    }
}

/// Trait-dispatch core of [`is_accelerable`]: does the named type's definition
/// list the `Accelerable` trait? Only class definitions carry a trait list; the
/// stdlib `Array` / `List` classes opt in via `implements ... Accelerable`.
fn type_implements_accelerable(
    name: &str,
    type_definitions: &std::collections::HashMap<String, TypeDefinition>,
) -> bool {
    match type_definitions.get(name) {
        Some(TypeDefinition::Class(def)) => def
            .traits
            .iter()
            .any(|trait_name| trait_name == ACCELERABLE_TRAIT_NAME),
        _ => false,
    }
}

/// Every type-valued generic argument must itself be accelerable. Value generics
/// (e.g. the `Size` of `Array<T, Size>`) are not types and impose no constraint.
fn type_args_are_accelerable(
    args: Option<&[crate::ast::expression::Expression]>,
    type_definitions: &std::collections::HashMap<String, TypeDefinition>,
    allow_generic: bool,
) -> bool {
    let Some(args) = args else { return true };
    args.iter().all(|arg| match &arg.node {
        ExpressionKind::Type(ty, _) => accelerable_inner(&ty.kind, type_definitions, allow_generic),
        _ => true,
    })
}

fn expr_type_is_accelerable(
    expr: &crate::ast::expression::Expression,
    type_definitions: &std::collections::HashMap<String, TypeDefinition>,
    allow_generic: bool,
) -> bool {
    match &expr.node {
        ExpressionKind::Type(ty, _) => accelerable_inner(&ty.kind, type_definitions, allow_generic),
        _ => false,
    }
}

fn first_type_arg_is_gpu_compatible(args: Option<&[crate::ast::expression::Expression]>) -> bool {
    let Some(args) = args else { return false };
    let Some(first) = args.first() else {
        return false;
    };
    first_expr_type_is_gpu_compatible(first)
}

/// After type resolution, generic args are wrapped as `ExpressionKind::Type`.
/// Any other shape means the arg was never resolved — treat as not GPU-compatible
/// so the bug surfaces loudly instead of silently admitting unknown types.
fn first_expr_type_is_gpu_compatible(expr: &crate::ast::expression::Expression) -> bool {
    if let ExpressionKind::Type(ty, _) = &expr.node {
        is_gpu_compatible(&ty.kind)
    } else {
        false
    }
}

/// Determines whether a scalar type is permitted as the element of a WGSL
/// storage buffer.
///
/// Stricter than [`is_gpu_compatible`]: `Boolean` is a valid scalar for kernel
/// locals (WGSL `bool`) but WGSL forbids `bool` in `var<storage>` bindings, so
/// an `Array<Boolean, N>` captured by a `gpu for` would round-trip as invalid
/// shader source. The other rejected types are non-scalar or have no fixed
/// runtime representation on the device.
///
/// Returns `false` for `Generic` so an unresolved generic element type cannot
/// silently slip through as a buffer element; the instantiation site is where
/// the concrete element must be checked.
pub fn is_gpu_buffer_element(kind: &TypeKind) -> bool {
    match kind {
        TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::Float
        | TypeKind::F32
        | TypeKind::F64 => true,
        TypeKind::I128
        | TypeKind::U128
        | TypeKind::Boolean
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::String
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Tuple(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Option(_)
        | TypeKind::Linear(_)
        | TypeKind::Meta(_)
        | TypeKind::RawPtr
        | TypeKind::Identifier
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Custom(_, _) => false,
    }
}

/// Returns the element type spelling and a human-readable kind label for a
/// captured collection-shaped type that would lower to a WGSL storage buffer.
///
/// Recognizes the canonical `TypeKind::Array(elem, _)` / `TypeKind::List(elem)`
/// shapes and the post-resolution `TypeKind::Custom(name, args)` envelopes for
/// the builtin `Array` and `List` collections (looked up via
/// `BuiltinCollectionKind::from_name`).
///
/// Returns `None` for non-collection types — the caller treats those as
/// non-buffer captures and is responsible for the (scalar-by-scalar) GPU
/// compatibility check that already runs over the body.
pub fn captured_buffer_element(kind: &TypeKind) -> Option<Type> {
    match kind {
        TypeKind::Array(elem_expr, _) | TypeKind::List(elem_expr) => {
            extract_element_type(elem_expr)
        }
        TypeKind::Custom(name, Some(args)) => {
            let is_collection = matches!(
                BuiltinCollectionKind::from_name(name),
                Some(BuiltinCollectionKind::Array) | Some(BuiltinCollectionKind::List)
            );
            if !is_collection {
                return None;
            }
            args.first().and_then(extract_element_type)
        }
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
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::Boolean
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::String
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Tuple(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Option(_)
        | TypeKind::Linear(_)
        | TypeKind::Meta(_)
        | TypeKind::RawPtr
        | TypeKind::Identifier
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Custom(_, None) => None,
    }
}

/// Returns `true` for capture types the GPU dispatcher marshals as a plain
/// `var<storage>` binding from a host `MiriArray`-shaped buffer: fixed-size
/// `Array<T, N>` / `[T; N]`.
///
/// Kept in lock-step with `gpu_for::is_gpu_buffer_capture` (the MIR predicate
/// that decides what actually becomes a storage binding). `List<T>` is dynamic
/// and has no fixed device storage layout — it can never be a `gpu for`
/// capture, so annotating it with `gpu let` would not help; it is rejected as
/// a non-buffer capture at MIR lowering instead.
///
/// The residency capture rule therefore governs only the plain `Array`
/// captures a `gpu let` can produce.
pub fn is_residency_gated_buffer(kind: &TypeKind) -> bool {
    match kind {
        TypeKind::Array(_, _) => true,
        TypeKind::Custom(name, _) => {
            BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Array)
        }
        TypeKind::List(_)
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
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::Boolean
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::String
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Tuple(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Option(_)
        | TypeKind::Linear(_)
        | TypeKind::Meta(_)
        | TypeKind::RawPtr
        | TypeKind::Identifier
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _) => false,
    }
}

fn extract_element_type(expr: &crate::ast::expression::Expression) -> Option<Type> {
    if let ExpressionKind::Type(ty, _) = &expr.node {
        Some((**ty).clone())
    } else {
        None
    }
}

// Re-export the shared element-type resolver from ast::types so it is available
// to type checking code. The function is defined once in src/ast/types.rs to
// prevent duplication with src/mir/lowering/gpu_for.rs.
pub use crate::ast::types::resolve_element_type_kind;

/// Determines whether a type is auto-copy given available type definitions.
///
/// A type is auto-copy when:
/// - It is a primitive (int, float, bool, i8..i128, u8..u128, f32, f64, void)
/// - It is a struct/enum whose **all** fields are themselves auto-copy, and
///   the total estimated size is ≤ `AUTO_COPY_MAX_SIZE` (128 bytes)
/// - Tuples of auto-copy types
///
/// Managed types (String, List, Array, Map, Set, classes) are never auto-copy.
pub fn is_auto_copy<'a>(
    kind: &'a TypeKind,
    type_definitions: &'a std::collections::HashMap<String, TypeDefinition>,
) -> bool {
    is_auto_copy_inner(
        kind,
        type_definitions,
        &mut std::collections::HashSet::new(),
    )
}

/// Recursive helper with a visited set to prevent infinite recursion on cyclic types.
fn is_auto_copy_inner<'a>(
    kind: &'a TypeKind,
    type_definitions: &'a std::collections::HashMap<String, TypeDefinition>,
    visited: &mut std::collections::HashSet<&'a str>,
) -> bool {
    match kind {
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
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::Boolean
        | TypeKind::RawPtr
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Identifier
        | TypeKind::Function(_) => true,

        TypeKind::String
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Meta(_)
        | TypeKind::Linear(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_) => false,

        TypeKind::Tuple(elements) => is_auto_copy_tuple(elements, type_definitions, visited),

        TypeKind::Option(inner) => is_auto_copy_inner(&inner.kind, type_definitions, visited),

        TypeKind::Custom(name, _) => is_auto_copy_custom(name, kind, type_definitions, visited),
    }
}

fn is_auto_copy_tuple<'a>(
    elements: &'a [crate::ast::expression::Expression],
    type_definitions: &'a std::collections::HashMap<String, TypeDefinition>,
    visited: &mut std::collections::HashSet<&'a str>,
) -> bool {
    elements.iter().all(|elem_expr| {
        if let crate::ast::expression::ExpressionKind::Type(ty, _) = &elem_expr.node {
            is_auto_copy_inner(&ty.kind, type_definitions, visited)
        } else {
            false
        }
    })
}

fn is_auto_copy_custom<'a>(
    name: &'a str,
    kind: &'a TypeKind,
    type_definitions: &'a std::collections::HashMap<String, TypeDefinition>,
    visited: &mut std::collections::HashSet<&'a str>,
) -> bool {
    if !visited.insert(name) {
        return false;
    }

    match type_definitions.get(name) {
        Some(TypeDefinition::Struct(struct_def)) => {
            is_auto_copy_struct(struct_def, kind, type_definitions, visited)
        }
        Some(TypeDefinition::Enum(enum_def)) => {
            is_auto_copy_enum(enum_def, kind, type_definitions, visited)
        }
        Some(TypeDefinition::Alias(alias_def)) => {
            is_auto_copy_inner(&alias_def.template.kind, type_definitions, visited)
        }
        Some(TypeDefinition::Class(_))
        | Some(TypeDefinition::Trait(_))
        | Some(TypeDefinition::Generic(_))
        | None => false,
    }
}

fn is_auto_copy_struct<'a>(
    struct_def: &'a crate::type_checker::context::StructDefinition,
    kind: &TypeKind,
    type_definitions: &'a std::collections::HashMap<String, TypeDefinition>,
    visited: &mut std::collections::HashSet<&'a str>,
) -> bool {
    if struct_def.has_drop {
        return false;
    }
    let all_fields_copy = struct_def
        .fields
        .iter()
        .all(|(_, field_ty, _)| is_auto_copy_inner(&field_ty.kind, type_definitions, visited));
    if !all_fields_copy {
        return false;
    }
    estimated_type_size(kind, type_definitions) <= crate::mir::body::AUTO_COPY_MAX_SIZE
}

fn is_auto_copy_enum<'a>(
    enum_def: &'a crate::type_checker::context::EnumDefinition,
    kind: &TypeKind,
    type_definitions: &'a std::collections::HashMap<String, TypeDefinition>,
    visited: &mut std::collections::HashSet<&'a str>,
) -> bool {
    let all_variants_copy = enum_def.variants.values().all(|payload_types| {
        payload_types
            .iter()
            .all(|ty| is_auto_copy_inner(&ty.kind, type_definitions, visited))
    });
    if !all_variants_copy {
        return false;
    }
    estimated_type_size(kind, type_definitions) <= crate::mir::body::AUTO_COPY_MAX_SIZE
}

/// Estimates the byte size of a type for auto-copy threshold checking.
///
/// Returns a conservative (possibly over-) estimate. Uses 8 bytes as a
/// default for pointer-sized/unknown types.
fn estimated_type_size(
    kind: &TypeKind,
    type_definitions: &std::collections::HashMap<String, TypeDefinition>,
) -> usize {
    match kind {
        TypeKind::I8 | TypeKind::U8 | TypeKind::Boolean => 1,
        TypeKind::I16 | TypeKind::U16 => 2,
        TypeKind::I32 | TypeKind::U32 | TypeKind::F32 => 4,
        TypeKind::Int
        | TypeKind::I64
        | TypeKind::U64
        | TypeKind::Float
        | TypeKind::F64
        | TypeKind::RawPtr => 8,
        TypeKind::I128 | TypeKind::U128 => 16,
        TypeKind::Custom(name, _) => match type_definitions.get(name) {
            Some(TypeDefinition::Struct(struct_def)) => struct_def
                .fields
                .iter()
                .map(|(_, ty, _)| estimated_type_size(&ty.kind, type_definitions))
                .sum(),
            Some(TypeDefinition::Enum(enum_def)) => {
                // discriminant (8) + max payload size
                let max_payload: usize = enum_def
                    .variants
                    .values()
                    .map(|fields| {
                        fields
                            .iter()
                            .map(|ty| estimated_type_size(&ty.kind, type_definitions))
                            .sum::<usize>()
                    })
                    .max()
                    .unwrap_or(0);
                8 + max_payload
            }
            _ => 8,
        },
        TypeKind::Tuple(elements) => elements
            .iter()
            .map(|elem_expr| {
                if let crate::ast::expression::ExpressionKind::Type(ty, _) = &elem_expr.node {
                    estimated_type_size(&ty.kind, type_definitions)
                } else {
                    8
                }
            })
            .sum(),
        _ => 8,
    }
}

impl TypeChecker {
    // ==================== Visible Type Resolution ====================

    /// Registers a type definition and marks it as visible to user code.
    ///
    /// All type registrations should go through this method so that
    /// `resolve_visible_type` works correctly.
    pub(crate) fn register_type_definition(&mut self, name: String, def: TypeDefinition) {
        self.visible_type_names.insert(name.clone());
        self.global_type_definitions.insert(name, def);
    }

    /// Resolves a type definition that is visible from user code.
    ///
    /// Use this for **user-facing** name resolution: `implements`, `extends`,
    /// type annotations, constructor calls, pattern matching, etc.
    ///
    /// Checks scoped generics (from context) first, then global types gated by
    /// `visible_type_names`. For **internal** lookups where the type is already
    /// known to exist (walking inheritance chains, vtable resolution, method
    /// signature checking), use `global_type_definitions` directly.
    pub(crate) fn resolve_visible_type<'a>(
        &'a self,
        name: &str,
        context: &'a Context,
    ) -> Option<&'a TypeDefinition> {
        // Generic type parameters are scoped — they live only in context,
        // never in global_type_definitions.
        if let Some(def @ TypeDefinition::Generic(_)) = context.resolve_type_definition(name) {
            return Some(def);
        }
        if self.visible_type_names.contains(name) {
            self.global_type_definitions.get(name)
        } else {
            None
        }
    }

    /// Returns true if the named type is visible from user code.
    pub(crate) fn is_type_visible(&self, name: &str) -> bool {
        self.visible_type_names.contains(name)
    }

    // ==================== Error Type Helper ====================

    /// Creates an error type. Use this when type checking fails.
    #[inline]
    pub(crate) fn error_type() -> Type {
        make_type(TypeKind::Error)
    }

    // ==================== Type Predicates ====================

    /// Checks if a type is numeric (any integer or float type).
    pub(crate) fn is_numeric(&self, t: &Type) -> bool {
        matches!(
            t.kind,
            TypeKind::Int
                | TypeKind::Float
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
                | TypeKind::F32
                | TypeKind::F64
        )
    }

    /// Checks if a type is an integer type.
    pub(crate) fn is_integer(&self, t: &Type) -> bool {
        matches!(
            t.kind,
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
        )
    }

    /// Returns the bit size of an integer type, or None if not an integer.
    pub(crate) fn get_integer_size(&self, t: &Type) -> Option<u8> {
        match &t.kind {
            TypeKind::I8 | TypeKind::U8 => Some(8),
            TypeKind::I16 | TypeKind::U16 => Some(16),
            TypeKind::I32 | TypeKind::U32 => Some(32),
            TypeKind::I64 | TypeKind::U64 => Some(64),
            TypeKind::I128 | TypeKind::U128 => Some(128),
            TypeKind::Int => Some(128), // Treat literal Int as max size for compatibility
            _ => None,
        }
    }

    // ==================== Visibility Checking ====================

    /// Checks if a symbol with the given visibility is accessible from the current module.
    pub(crate) fn check_visibility(&self, visibility: &MemberVisibility, module: &str) -> bool {
        match visibility {
            MemberVisibility::Public => true,
            MemberVisibility::Private => module == self.current_module,
            MemberVisibility::Protected => {
                module == self.current_module || self.is_subtype(&self.current_module, module)
            }
        }
    }

    /// Checks if a class member can be accessed from the current context.
    ///
    /// - `public`: always accessible.
    /// - `private`: only accessible from within the declaring class itself.
    /// - `protected`: accessible from the declaring class and its subclasses,
    ///   **but only through a receiver whose declared type is also a subtype of
    ///   the current class**. This prevents sibling-class access: if `Cat` and
    ///   `Dog` both extend `Animal`, a method on `Cat` must not read `dog.field`
    ///   even when `field` is declared `protected` on `Animal`.
    ///
    /// # Parameters
    /// - `member_class`: the class that declares the member.
    /// - `current_class`: the class in whose method body the access occurs.
    /// - `receiver_class`: the declared type of the receiver expression. For
    ///   self-access this equals `current_class`; for external receivers it is
    ///   the type of the object being accessed.
    pub(crate) fn check_member_visibility(
        &self,
        visibility: &MemberVisibility,
        member_class: &str,
        current_class: Option<&str>,
        receiver_class: Option<&str>,
    ) -> bool {
        match visibility {
            MemberVisibility::Public => true,
            MemberVisibility::Private => current_class == Some(member_class),
            MemberVisibility::Protected => {
                if let Some(curr) = current_class {
                    // The current class must be in the member's inheritance subtree.
                    let owns_member = curr == member_class || self.is_subtype(curr, member_class);

                    // For external receiver access the current class must also be a
                    // subtype of the receiver's declared type (Java-style rule).
                    // This blocks sibling access: Cat is not a subtype of Dog.
                    let can_reach_receiver = match receiver_class {
                        Some(recv) if recv != curr => curr == recv || self.is_subtype(curr, recv),
                        _ => true, // self-access or same-class: no extra restriction
                    };

                    owns_member && can_reach_receiver
                } else {
                    false
                }
            }
        }
    }

    // ==================== Type Expression Helpers ====================

    /// Creates a type expression from a Type.
    pub(crate) fn create_type_expression(&self, ty: Type) -> Expression {
        IdNode::new(
            0,
            ExpressionKind::Type(Box::new(ty), false),
            Span::new(0, 0),
        )
    }

    /// Extracts the element type from an iterable type.
    ///
    /// Supports: List<T>, Set<T>, Map<K,V>, String, Range<T>
    pub(crate) fn get_iterable_element_type(&mut self, ty: &Type, span: Span) -> Type {
        match &ty.kind {
            TypeKind::String => make_type(TypeKind::String),
            // Collection canonical variants are normalized to Custom before type-checking.
            TypeKind::List(_) | TypeKind::Array(_, _) | TypeKind::Set(_) | TypeKind::Map(_, _) => {
                unreachable!("collection types are normalized to Custom before this point")
            }
            TypeKind::Tuple(element_type_exprs) => {
                // For homogeneous tuples, return the element type
                if element_type_exprs.is_empty() {
                    Self::error_type()
                } else {
                    self.extract_type_from_expression(&element_type_exprs[0])
                        .unwrap_or_else(|_| Self::error_type())
                }
            }
            TypeKind::Custom(name, args)
                if BuiltinCollectionKind::from_name(name).is_some() || name == "Tuple" =>
            {
                if let Some(args) = args {
                    if !args.is_empty() {
                        return self
                            .extract_type_from_expression(&args[0])
                            .unwrap_or_else(|_| Self::error_type());
                    }
                } else {
                    // Inside the class definition itself, args is None.
                    // We can look up the generic parameter 'T' from the context.
                    // To do this, we need the context, but this method currently doesn't take context.
                    // Wait, this method only takes ty and span. It doesn't take context!
                    // Let's just return a generic 'T'.
                    return make_type(TypeKind::Generic(
                        "T".to_string(),
                        None,
                        TypeDeclarationKind::None,
                    ));
                }
                Self::error_type()
            }
            TypeKind::Custom(name, args) if name == "Range" => {
                if let Some(args) = args {
                    if let Some(arg) = args.first() {
                        return self
                            .extract_type_from_expression(arg)
                            .unwrap_or_else(|_| Self::error_type());
                    }
                }
                Self::error_type()
            }
            TypeKind::Error => Self::error_type(),
            _ => {
                self.report_error(format!("Type {} is not iterable", ty), span);
                Self::error_type()
            }
        }
    }

    // ==================== Name and Type Extraction ====================

    /// Extracts a name from an identifier expression.
    pub(crate) fn extract_name<'a>(&self, expr: &'a Expression) -> Result<&'a str, String> {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => Ok(name.as_str()),
            _ => Err("Expected identifier".to_string()),
        }
    }

    /// Extracts a type name from an expression (identifier or type expression).
    pub(crate) fn extract_type_name<'a>(&self, expr: &'a Expression) -> Result<&'a str, String> {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => Ok(name.as_str()),
            ExpressionKind::Type(ty, _) => match &ty.kind {
                TypeKind::Custom(name, _) => Ok(name.as_str()),
                _ => Err("Expected custom type".to_string()),
            },
            // `inheritance_identifier` emits TypeDeclaration for `ClassName<T>` in
            // `extends` / `implements` clauses.  Extract the base name from the inner
            // identifier expression.
            ExpressionKind::TypeDeclaration(inner, _, _, _) => {
                if let ExpressionKind::Identifier(name, _) = &inner.node {
                    Ok(name.as_str())
                } else {
                    Err("Expected identifier in type declaration".to_string())
                }
            }
            _ => Err("Expected type identifier".to_string()),
        }
    }

    /// Extracts a Type from a type expression.
    pub(crate) fn extract_type_from_expression(&self, expr: &Expression) -> Result<Type, String> {
        match &expr.node {
            ExpressionKind::Type(t, is_nullable) => {
                if *is_nullable {
                    Ok(make_type(TypeKind::Option(t.clone())))
                } else {
                    Ok(*t.clone())
                }
            }
            _ => Err("Expected type expression".to_string()),
        }
    }

    // ==================== Type Resolution ====================

    /// Resolves a type expression to a concrete Type.
    ///
    /// Handles:
    /// - Built-in collection types (List, Set, Map, Range)
    /// - Option types
    /// - Custom types with generic arguments
    /// - Type aliases
    /// - Generic type parameters
    pub(crate) fn resolve_type_expression(&mut self, expr: &Expression, context: &Context) -> Type {
        match self.extract_type_from_expression(expr) {
            Ok(t) => self.resolve_type_kind(t, expr, context),
            Err(msg) => {
                self.report_error(msg, expr.span);
                Self::error_type()
            }
        }
    }

    /// Resolves a Type based on its kind.
    fn resolve_type_kind(&mut self, t: Type, expr: &Expression, context: &Context) -> Type {
        match t.kind {
            TypeKind::List(inner) => self.resolve_list_type(inner, context),
            TypeKind::Set(inner) => self.resolve_set_type(inner, context),
            TypeKind::Map(k, v) => self.resolve_map_type(k, v, context),
            TypeKind::Option(inner) => self.resolve_option_type(*inner, context),
            TypeKind::Array(inner, size) => self.resolve_array_type(inner, size, context),
            TypeKind::Result(ok, err) => self.resolve_result_type(ok, err, context),
            TypeKind::Custom(name, args) => self.resolve_custom_type(&name, args, expr, context),
            TypeKind::Tuple(elements) => self.resolve_tuple_type(elements, context),
            _ => make_type(t.kind),
        }
    }

    fn resolve_list_type(&mut self, inner: Box<Expression>, context: &Context) -> Type {
        let resolved_inner = self.resolve_type_expression(&inner, context);
        make_type(TypeKind::Custom(
            "List".to_string(),
            Some(vec![self.create_type_expression(resolved_inner)]),
        ))
    }

    fn resolve_set_type(&mut self, inner: Box<Expression>, context: &Context) -> Type {
        let resolved_inner = self.resolve_type_expression(&inner, context);
        if let TypeKind::Option(_) = resolved_inner.kind {
            self.report_error("Set elements cannot be optional".to_string(), inner.span);
        }
        make_type(TypeKind::Custom(
            "Set".to_string(),
            Some(vec![self.create_type_expression(resolved_inner)]),
        ))
    }

    fn resolve_map_type(
        &mut self,
        k: Box<Expression>,
        v: Box<Expression>,
        context: &Context,
    ) -> Type {
        let rk = self.resolve_type_expression(&k, context);
        if let TypeKind::Option(_) = rk.kind {
            self.report_error("Map keys cannot be optional".to_string(), k.span);
        }
        let rv = self.resolve_type_expression(&v, context);
        make_type(TypeKind::Custom(
            "Map".to_string(),
            Some(vec![
                self.create_type_expression(rk),
                self.create_type_expression(rv),
            ]),
        ))
    }

    fn resolve_option_type(&mut self, inner: Type, context: &Context) -> Type {
        let inner_expr = self.create_type_expression(inner);
        let resolved_inner = self.resolve_type_expression(&inner_expr, context);
        make_type(TypeKind::Option(Box::new(resolved_inner)))
    }

    fn resolve_array_type(
        &mut self,
        inner: Box<Expression>,
        size: Box<Expression>,
        context: &Context,
    ) -> Type {
        let resolved_inner = self.resolve_type_expression(&inner, context);
        let folded_size = if let Some(val) = Self::try_eval_const_int(&size) {
            Box::new(crate::ast::factory::int_literal_expression(val))
        } else {
            size
        };
        make_type(TypeKind::Custom(
            "Array".to_string(),
            Some(vec![
                self.create_type_expression(resolved_inner),
                *folded_size,
            ]),
        ))
    }

    fn resolve_result_type(
        &mut self,
        ok: Box<Expression>,
        err: Box<Expression>,
        context: &Context,
    ) -> Type {
        let ok_type = self.resolve_type_expression(&ok, context);
        let err_type = self.resolve_type_expression(&err, context);
        make_type(TypeKind::Custom(
            "Result".to_string(),
            Some(vec![
                self.create_type_expression(ok_type),
                self.create_type_expression(err_type),
            ]),
        ))
    }

    fn resolve_tuple_type(&mut self, elements: Vec<Expression>, context: &Context) -> Type {
        let resolved_elements: Vec<Expression> = elements
            .iter()
            .map(|elem_expr| {
                let resolved = self.resolve_type_expression(elem_expr, context);
                self.create_type_expression(resolved)
            })
            .collect();
        make_type(TypeKind::Tuple(resolved_elements))
    }

    /// Resolves a custom type (user-defined or built-in generic type).
    fn resolve_custom_type(
        &mut self,
        name: &str,
        args: Option<Vec<Expression>>,
        expr: &Expression,
        context: &Context,
    ) -> Type {
        // Resolve `Self` to the current class/trait type
        if name == "Self" {
            if let Some(class_type) = &context.current_class_type {
                return class_type.clone();
            }
            self.report_error(
                "'Self' can only be used inside a class or trait".to_string(),
                expr.span,
            );
            return Self::error_type();
        }

        // Handle built-in generic type aliases
        if let Some(resolved) = self.resolve_builtin_type_alias(name, &args, context) {
            return resolved;
        }

        // Resolve generic arguments recursively
        let resolved_args = args.map(|args_vec| {
            args_vec
                .iter()
                .map(|arg| {
                    let resolved_type = self.resolve_type_expression(arg, context);
                    self.create_type_expression(resolved_type)
                })
                .collect()
        });

        // Look up type definition (user-facing: must be visible in scope)
        if let Some(def) = self.resolve_visible_type(name, context).cloned() {
            // Types used purely as annotations (e.g. `private trait Foo` in a
            // parameter position) never go through the identifier-lookup path
            // that enforces `check_visibility`.  We close that gap here: if the
            // type name also has a symbol-table entry (all user-defined types do)
            // we check its top-level visibility now.
            if let Some(sym) = self.global_scope.get(name) {
                if !self.check_visibility(&sym.visibility, &sym.module) {
                    self.report_error(format!("Type '{}' is not visible", name), expr.span);
                    return Self::error_type();
                }
            }
            self.validate_and_resolve_type_definition(name, def, resolved_args, expr, context)
        } else {
            self.report_unknown_type(name, expr, context);
            Self::error_type()
        }
    }

    /// Resolves built-in type aliases like Map<K,V>, List<T>, Set<T>, Range<T>.
    fn resolve_builtin_type_alias(
        &mut self,
        name: &str,
        args: &Option<Vec<Expression>>,
        context: &Context,
    ) -> Option<Type> {
        match name {
            "Map" => self.resolve_alias_map(args, context),
            "Array" => self.resolve_alias_array(args, context),
            "List" | "list" => self.resolve_alias_list(args, context),
            "Set" | "set" => self.resolve_alias_set(args, context),
            "range" => self.resolve_alias_range(args, context),
            "Option" => self.resolve_alias_option(args, context),
            "Linear" => self.resolve_alias_linear(args, context),
            _ => None,
        }
    }

    fn resolve_alias_map(
        &mut self,
        args: &Option<Vec<Expression>>,
        context: &Context,
    ) -> Option<Type> {
        let args = args.as_ref()?;
        if args.len() != 2 {
            return None;
        }
        let k = self.resolve_type_expression(&args[0], context);
        if let TypeKind::Option(_) = k.kind {
            self.report_error("Map keys cannot be optional".to_string(), args[0].span);
        }
        let v = self.resolve_type_expression(&args[1], context);
        Some(make_type(TypeKind::Custom(
            "Map".to_string(),
            Some(vec![
                self.create_type_expression(k),
                self.create_type_expression(v),
            ]),
        )))
    }

    fn resolve_alias_array(
        &mut self,
        args: &Option<Vec<Expression>>,
        context: &Context,
    ) -> Option<Type> {
        let args = args.as_ref()?;
        if args.len() != 2 {
            return None;
        }
        let elem = self.resolve_type_expression(&args[0], context);
        let size = &args[1];
        let folded_size = if let Some(val) = Self::try_eval_const_int(size) {
            Box::new(crate::ast::factory::int_literal_expression(val))
        } else {
            Box::new(size.clone())
        };
        Some(make_type(TypeKind::Custom(
            "Array".to_string(),
            Some(vec![self.create_type_expression(elem), *folded_size]),
        )))
    }

    fn resolve_alias_list(
        &mut self,
        args: &Option<Vec<Expression>>,
        context: &Context,
    ) -> Option<Type> {
        let args = args.as_ref()?;
        if args.len() != 1 {
            return None;
        }
        let t = self.resolve_type_expression(&args[0], context);
        Some(make_type(TypeKind::Custom(
            "List".to_string(),
            Some(vec![self.create_type_expression(t)]),
        )))
    }

    fn resolve_alias_set(
        &mut self,
        args: &Option<Vec<Expression>>,
        context: &Context,
    ) -> Option<Type> {
        let args = args.as_ref()?;
        if args.len() != 1 {
            return None;
        }
        let t = self.resolve_type_expression(&args[0], context);
        if let TypeKind::Option(_) = t.kind {
            self.report_error("Set elements cannot be optional".to_string(), args[0].span);
        }
        Some(make_type(TypeKind::Custom(
            "Set".to_string(),
            Some(vec![self.create_type_expression(t)]),
        )))
    }

    fn resolve_alias_range(
        &mut self,
        args: &Option<Vec<Expression>>,
        context: &Context,
    ) -> Option<Type> {
        match args {
            Some(args) if args.len() == 1 => {
                let t = self.resolve_type_expression(&args[0], context);
                Some(make_type(TypeKind::Custom(
                    "Range".to_string(),
                    Some(vec![self.create_type_expression(t)]),
                )))
            }
            None => Some(make_type(TypeKind::Custom(
                "Range".to_string(),
                Some(vec![self.create_type_expression(make_type(TypeKind::Int))]),
            ))),
            _ => None,
        }
    }

    fn resolve_alias_option(
        &mut self,
        args: &Option<Vec<Expression>>,
        context: &Context,
    ) -> Option<Type> {
        let args = args.as_ref()?;
        if args.len() != 1 {
            return None;
        }
        let t = self.resolve_type_expression(&args[0], context);
        Some(make_type(TypeKind::Option(Box::new(t))))
    }

    fn resolve_alias_linear(
        &mut self,
        args: &Option<Vec<Expression>>,
        context: &Context,
    ) -> Option<Type> {
        let args = args.as_ref()?;
        if args.len() != 1 {
            return None;
        }
        let t = self.resolve_type_expression(&args[0], context);
        Some(make_type(TypeKind::Linear(Box::new(t))))
    }

    /// Validates a type definition and returns the resolved type.
    fn validate_and_resolve_type_definition(
        &mut self,
        name: &str,
        def: TypeDefinition,
        resolved_args: Option<Vec<Expression>>,
        expr: &Expression,
        context: &Context,
    ) -> Type {
        match def {
            TypeDefinition::Struct(struct_def) => {
                self.validate_generics(&resolved_args, &struct_def.generics, context, expr.span);
                make_type(TypeKind::Custom(name.to_string(), resolved_args))
            }
            TypeDefinition::Enum(enum_def) => {
                self.validate_generics(&resolved_args, &enum_def.generics, context, expr.span);
                make_type(TypeKind::Custom(name.to_string(), resolved_args))
            }
            TypeDefinition::Generic(gen_def) => {
                if resolved_args.is_some() {
                    self.report_error(
                        "Generic type parameter cannot have generic arguments".to_string(),
                        expr.span,
                    );
                }
                make_type(TypeKind::Generic(
                    name.to_string(),
                    gen_def.constraint.clone().map(Box::new),
                    gen_def.kind,
                ))
            }
            TypeDefinition::Alias(alias_def) => {
                self.resolve_type_alias(name, alias_def, resolved_args, expr, context)
            }
            TypeDefinition::Class(class_def) => {
                self.validate_generics(&resolved_args, &class_def.generics, context, expr.span);
                make_type(TypeKind::Custom(name.to_string(), resolved_args))
            }
            TypeDefinition::Trait(trait_def) => {
                self.validate_generics(&resolved_args, &trait_def.generics, context, expr.span);
                make_type(TypeKind::Custom(name.to_string(), resolved_args))
            }
        }
    }

    /// Resolves a type alias with generic substitution.
    fn resolve_type_alias(
        &mut self,
        name: &str,
        alias_def: super::context::AliasDefinition,
        resolved_args: Option<Vec<Expression>>,
        expr: &Expression,
        _context: &Context,
    ) -> Type {
        let expected_count = alias_def.generics.as_ref().map_or(0, |g| g.len());
        let provided_count = resolved_args.as_ref().map_or(0, |a| a.len());

        if expected_count != provided_count {
            self.report_generic_count_mismatch(name, expected_count, provided_count, expr);
            return Self::error_type();
        }

        // Substitute generic parameters
        if let Some(gen_defs) = &alias_def.generics {
            let mut mapping = std::collections::HashMap::new();
            if let Some(args) = &resolved_args {
                for (gen_def, arg_expr) in gen_defs.iter().zip(args.iter()) {
                    let arg_type = self
                        .extract_type_from_expression(arg_expr)
                        .unwrap_or_else(|_| Self::error_type());
                    mapping.insert(gen_def.name.clone(), arg_type);
                }
            }
            return self.substitute_type(&alias_def.template, &mapping);
        }

        alias_def.template.clone()
    }

    /// Reports a generic argument count mismatch error.
    fn report_generic_count_mismatch(
        &mut self,
        name: &str,
        expected: usize,
        provided: usize,
        expr: &Expression,
    ) {
        let message = if expected == 0 && provided > 0 {
            format!(
                "Type alias '{}' is not generic but {} type argument(s) were provided",
                name, provided
            )
        } else if provided == 0 && expected > 0 {
            format!(
                "Type alias '{}' requires {} type argument(s)",
                name, expected
            )
        } else {
            format!(
                "Type alias '{}' expects {} type argument(s), got {}",
                name, expected, provided
            )
        };
        self.report_error(message, expr.span);
    }

    /// Reports an unknown type error with suggestions.
    fn report_unknown_type(&mut self, name: &str, expr: &Expression, context: &Context) {
        let capacity = context
            .type_definitions
            .iter()
            .map(|s| s.len())
            .sum::<usize>()
            + self.global_type_definitions.len()
            + 6;
        let mut candidates: Vec<&str> = Vec::with_capacity(capacity);
        for scope in &context.type_definitions {
            candidates.extend(scope.keys().map(|s| s.as_str()));
        }
        candidates.extend(self.global_type_definitions.keys().map(|s| s.as_str()));
        candidates.extend(["Int", "Float", "String", "Bool", "Void", "Any"]);

        if let Some(suggestion) = find_best_match(name, &candidates) {
            self.report_error_with_help(
                format!("Unknown type: {}", name),
                expr.span,
                format!("Did you mean '{}'?", suggestion),
            );
        } else {
            self.report_error(format!("Unknown type: {}", name), expr.span);
        }
    }

    // ==================== Mutability Checking ====================

    /// Checks if an expression is mutable (can be assigned to).
    #[allow(clippy::only_used_in_recursion)]
    pub(crate) fn is_mutable_expression(&self, expr: &Expression, context: &Context) -> bool {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => {
                // 'self' is considered mutable for assignment purposes
                if name == "self" {
                    return true;
                }
                context.is_mutable(name)
            }
            ExpressionKind::Member(obj, prop) => {
                // For self.field, check field mutability
                if let ExpressionKind::Identifier(name, _) = &obj.node {
                    if name == "self" {
                        if let Some(class_name) = &context.current_class {
                            if let Some(TypeDefinition::Class(def)) =
                                self.global_type_definitions.get(class_name)
                            {
                                if let ExpressionKind::Identifier(field_name, _) = &prop.node {
                                    if let Some((_, field_info)) =
                                        def.fields.iter().find(|(n, _)| n == field_name)
                                    {
                                        return field_info.mutable;
                                    }
                                }
                            }
                        }
                        return true;
                    }
                }
                self.is_mutable_expression(obj, context)
            }
            ExpressionKind::Index(obj, _) => self.is_mutable_expression(obj, context),
            _ => false,
        }
    }

    // ==================== Constant Evaluation ====================

    /// Tries to evaluate a constant integer expression at compile time.
    ///
    /// Supports integer literals, unary negate/plus, and binary arithmetic
    /// operations on constant sub-expressions. Does not resolve identifiers.
    pub(crate) fn try_eval_const_int(expr: &Expression) -> Option<i128> {
        Self::eval_const_int_inner(expr, None)
    }

    /// Tries to evaluate a constant integer expression at compile time,
    /// with context for resolving constant identifiers.
    pub(crate) fn try_eval_const_int_with_context(
        expr: &Expression,
        context: &Context,
    ) -> Option<i128> {
        Self::eval_const_int_inner(expr, Some(context))
    }

    fn eval_const_int_inner(expr: &Expression, context: Option<&Context>) -> Option<i128> {
        match &expr.node {
            ExpressionKind::Literal(Literal::Integer(val)) => Some(val.to_i128()),
            ExpressionKind::Identifier(name, _) => {
                let ctx = context?;
                let info = ctx.resolve_info(name)?;
                if !info.is_constant {
                    return None;
                }
                match &info.value {
                    Some(Literal::Integer(val)) => Some(val.to_i128()),
                    _ => None,
                }
            }
            ExpressionKind::Unary(UnaryOp::Negate, inner) => {
                Self::eval_const_int_inner(inner, context).map(|v| -v)
            }
            ExpressionKind::Unary(UnaryOp::Plus, inner) => {
                Self::eval_const_int_inner(inner, context)
            }
            ExpressionKind::Binary(left, op, right) => {
                let l = Self::eval_const_int_inner(left, context)?;
                let r = Self::eval_const_int_inner(right, context)?;
                match op {
                    BinaryOp::Add => l.checked_add(r),
                    BinaryOp::Sub => l.checked_sub(r),
                    BinaryOp::Mul => l.checked_mul(r),
                    BinaryOp::Div => {
                        if r == 0 {
                            None
                        } else {
                            l.checked_div(r)
                        }
                    }
                    BinaryOp::Mod => {
                        if r == 0 {
                            None
                        } else {
                            l.checked_rem(r)
                        }
                    }
                    _ => None,
                }
            }
            ExpressionKind::Call(callee, args) => {
                if args.is_empty() {
                    if let ExpressionKind::Identifier(name, _) = &callee.node {
                        if let Some(ctx) = context {
                            if let Some(info) = ctx.resolve_info(name) {
                                if info.is_constant {
                                    if let Some(Literal::Integer(val)) = &info.value {
                                        return Some(val.to_i128());
                                    }
                                }
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    // ==================== Error Reporting ====================

    /// Reports a type error, deduplicating identical (message, span) pairs.
    /// Reports a syntax error from an imported module, preserving its original
    /// error code and title. The caller must set `current_source_override` before
    /// calling this so the error is attributed to the correct file.
    pub(crate) fn report_syntax_error(&mut self, syntax_err: &crate::error::syntax::SyntaxError) {
        let mut err = crate::error::type_error::TypeError::from_syntax_error(syntax_err);
        err.source_override = self.current_source_override.clone();
        let key = (format!("{}", syntax_err), syntax_err.span);
        if self.reported_errors.insert(key) {
            self.errors.push(err);
        }
    }

    /// Returns the binding name when `expr` is a bare identifier whose symbol
    /// is gpu-resident. Compound expressions, unresolved names, and host
    /// bindings yield `None`. Shared by the element-cross-read, host-call, and
    /// cross-residency-assignment validation.
    pub(crate) fn gpu_resident_identifier<'a>(
        &self,
        expr: &'a Expression,
        context: &Context,
    ) -> Option<&'a str> {
        let ExpressionKind::Identifier(name, None) = &expr.node else {
            return None;
        };
        match context.resolve_info(name)?.residency {
            BindingResidency::Gpu => Some(name.as_str()),
            BindingResidency::Host => None,
        }
    }

    pub(crate) fn report_error(&mut self, message: String, span: Span) {
        let key = (message.clone(), span);
        if self.reported_errors.insert(key) {
            let mut err = TypeError::custom(message, span, None);
            err.source_override = self.current_source_override.clone();
            self.errors.push(err);
        }
    }

    /// Reports a type error with a help message, deduplicating identical (message, span) pairs.
    pub(crate) fn report_error_with_help(&mut self, message: String, span: Span, help: String) {
        let key = (message.clone(), span);
        if self.reported_errors.insert(key) {
            let mut err = TypeError::custom(message, span, Some(help));
            err.source_override = self.current_source_override.clone();
            self.errors.push(err);
        }
    }

    /// Reports a type warning with an error code, title, message, and help text.
    pub(crate) fn report_warning(
        &mut self,
        code: &'static str,
        title: String,
        message: String,
        span: Span,
        help: Option<String>,
    ) {
        use crate::error::diagnostic::{Diagnostic, Severity};
        self.warnings.push(Diagnostic {
            severity: Severity::Warning,
            code: Some(code),
            title,
            message,
            span: Some(span),
            help,
            notes: Vec::new(),
            source_override: self.current_source_override.clone(),
        });
    }

    // ==================== Recursive Type Detection ====================

    /// Checks whether a field type contains the struct `target_name` directly
    /// (without going through an optional/pointer indirection), which would
    /// make the type infinitely sized.
    pub(crate) fn is_infinite_recursive_type(&self, target_name: &str, ty: &TypeKind) -> bool {
        let mut visited = std::collections::HashSet::new();
        self.contains_type_directly(target_name, ty, &mut visited)
    }

    fn contains_type_directly<'a>(
        &'a self,
        target_name: &str,
        ty: &'a TypeKind,
        visited: &mut std::collections::HashSet<&'a str>,
    ) -> bool {
        match ty {
            TypeKind::Custom(name, _) if name == target_name => true,
            TypeKind::Custom(name, _) => {
                if !visited.insert(name.as_str()) {
                    return false; // Already checked, avoid infinite loop
                }
                // Check if this custom type transitively contains target_name
                if let Some(TypeDefinition::Struct(def)) = self.global_type_definitions.get(name) {
                    def.fields.iter().any(|(_, field_ty, _)| {
                        self.contains_type_directly(target_name, &field_ty.kind, visited)
                    })
                } else {
                    false
                }
            }
            // Tuple fields are inline, so check them
            TypeKind::Tuple(elements) => elements.iter().any(|expr| {
                if let ExpressionKind::Type(t, _) = &expr.node {
                    self.contains_type_directly(target_name, &t.kind, visited)
                } else {
                    false
                }
            }),
            // Optional, List, Array, Set, Map use pointer indirection — safe
            TypeKind::Option(_)
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Set(_)
            | TypeKind::Map(_, _) => false,
            _ => false,
        }
    }
}
