// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR-level type representation.
//!
//! [`MirType`] is a resolved, AST-free type used in MIR analysis passes.
//!
//! The main difference from [`crate::ast::types::TypeKind`] is that collection
//! element types are stored as `Box<MirType>` rather than `Box<Expression>`.
//! This eliminates the layering violation where the Perceus pass had to
//! pattern-match on AST [`ExpressionKind`] nodes to determine element types.
//!
//! [`MirType`] is constructed from [`TypeKind`] when a [`LocalDecl`] is created
//! during MIR lowering, so no extra traversal is needed at analysis time.
//!
//! [`ExpressionKind`]: crate::ast::expression::ExpressionKind
//! [`TypeKind`]: crate::ast::types::TypeKind
//! [`LocalDecl`]: crate::mir::LocalDecl

use crate::ast::types::{BuiltinCollectionKind, TypeKind};
use std::collections::HashSet;

/// A resolved, AST-expression-free type for use in MIR analysis.
///
/// All collection element types (e.g. the `T` in `[T]`) are stored as
/// `Box<MirType>` rather than `Box<Expression>`, so analysis passes can
/// traverse the type tree without touching the AST.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MirType {
    // ── Primitive / auto-copy types ──────────────────────────────────────────
    Int,
    I8,
    I16,
    I32,
    I64,
    I128,
    U8,
    U16,
    U32,
    U64,
    U128,
    Float,
    F32,
    F64,
    Boolean,
    Void,
    Identifier,
    RawPtr,
    Error,

    // ── String ───────────────────────────────────────────────────────────────
    /// Heap-allocated via `Box`, but **not** RC-managed (no `alloc_with_rc`
    /// layout).  Included as its own variant so callers can distinguish it
    /// from user-defined types without string matching.
    String,

    // ── RC-managed collection types ──────────────────────────────────────────
    /// Dynamic list `[T]`.  Element type resolved from the AST type expression.
    List(Box<MirType>),
    /// Fixed-size array `[T; N]`.  Element type resolved; size is stored for
    /// completeness but not required by RC analysis.
    Array(Box<MirType>),
    /// Hash map `{K: V}`.  Key and value types both resolved.
    Map(Box<MirType>, Box<MirType>),
    /// Hash set `{T}`.  Element type resolved.
    Set(Box<MirType>),
    /// Tuple `(T0, T1, …)`.  Element types resolved.
    Tuple(Vec<MirType>),
    /// Result `result<T, E>`.  Both type arguments resolved.
    Result(Box<MirType>, Box<MirType>),
    /// Optional `T?` or `Option<T>`.  Inner type resolved.
    Option(Box<MirType>),
    /// Async future `future<T>`.  Inner type resolved.
    Future(Box<MirType>),

    // ── User-defined / opaque types ──────────────────────────────────────────
    /// A named user-defined type (struct, class, or enum).
    /// Generic arguments are not tracked — only the base name is needed for
    /// RC management decisions.
    Custom(String),

    /// A heap-allocated closure (function value with a captured environment).
    /// Closure structs are RC-managed: [malloc_ptr][RC][fn_ptr][cap0][cap1]...
    /// The closure local holds `payload_ptr` (the word past the RC header).
    Function,

    /// An unresolved generic type parameter (e.g. `T`, `K`, `V`).
    /// Generic parameters are never concrete heap objects, so this variant is
    /// never managed.
    Generic,

    /// A type that could not be resolved during MIR construction.
    /// Treated as non-managed to avoid false-positive IncRef/DecRef.
    Unknown,
}

impl MirType {
    /// Converts an AST [`TypeKind`] into a [`MirType`], resolving all
    /// collection element-type expressions into `MirType` values.
    ///
    /// This is called once per [`LocalDecl`] at MIR construction time.
    ///
    /// [`LocalDecl`]: crate::mir::LocalDecl
    pub fn from_type_kind(kind: &TypeKind) -> Self {
        match kind {
            TypeKind::Int => MirType::Int,
            TypeKind::I8 => MirType::I8,
            TypeKind::I16 => MirType::I16,
            TypeKind::I32 => MirType::I32,
            TypeKind::I64 => MirType::I64,
            TypeKind::I128 => MirType::I128,
            TypeKind::U8 => MirType::U8,
            TypeKind::U16 => MirType::U16,
            TypeKind::U32 => MirType::U32,
            TypeKind::U64 => MirType::U64,
            TypeKind::U128 => MirType::U128,
            TypeKind::Float => MirType::Float,
            TypeKind::F32 => MirType::F32,
            TypeKind::F64 => MirType::F64,
            TypeKind::String => MirType::String,
            TypeKind::Boolean => MirType::Boolean,
            TypeKind::Void => MirType::Void,
            TypeKind::Identifier => MirType::Identifier,
            TypeKind::RawPtr => MirType::RawPtr,
            TypeKind::Error => MirType::Error,
            // Canonical collection variants are normalized to Custom before MIR lowering.
            // They are handled below in the Custom arm.
            TypeKind::List(elem) => MirType::List(Box::new(Self::from_expr(elem))),
            TypeKind::Array(elem, _size) => MirType::Array(Box::new(Self::from_expr(elem))),
            TypeKind::Map(k, v) => {
                MirType::Map(Box::new(Self::from_expr(k)), Box::new(Self::from_expr(v)))
            }
            TypeKind::Set(elem) => MirType::Set(Box::new(Self::from_expr(elem))),
            TypeKind::Tuple(elems) => MirType::Tuple(elems.iter().map(Self::from_expr).collect()),
            TypeKind::Result(ok, err) => MirType::Result(
                Box::new(Self::from_expr(ok)),
                Box::new(Self::from_expr(err)),
            ),
            TypeKind::Option(inner) => MirType::Option(Box::new(Self::from_type_kind(&inner.kind))),
            TypeKind::Future(inner) => MirType::Future(Box::new(Self::from_expr(inner))),
            // Generic type parameters — never concrete managed types.
            TypeKind::Generic(_, _, _) => MirType::Generic,
            // Closures are heap-allocated and RC-managed.
            TypeKind::Function(_) => MirType::Function,
            TypeKind::Custom(name, args) => {
                // After normalization, builtin collections are Custom("List"/"Array"/...).
                // Map them back to the corresponding MirType collection variant, but only
                // when args is Some (instantiated). When args is None, the name appears as
                // an unresolved self-reference inside a stdlib class body — keep it as
                // MirType::Custom so the managed-type check excludes it correctly.
                match (BuiltinCollectionKind::from_name(name), args) {
                    (Some(BuiltinCollectionKind::List), Some(args)) => {
                        let elem = args
                            .first()
                            .map(Self::from_expr)
                            .unwrap_or(MirType::Unknown);
                        MirType::List(Box::new(elem))
                    }
                    (Some(BuiltinCollectionKind::Array), Some(args)) => {
                        let elem = args
                            .first()
                            .map(Self::from_expr)
                            .unwrap_or(MirType::Unknown);
                        MirType::Array(Box::new(elem))
                    }
                    (Some(BuiltinCollectionKind::Map), Some(args)) => {
                        let k = args
                            .first()
                            .map(Self::from_expr)
                            .unwrap_or(MirType::Unknown);
                        let v = args.get(1).map(Self::from_expr).unwrap_or(MirType::Unknown);
                        MirType::Map(Box::new(k), Box::new(v))
                    }
                    (Some(BuiltinCollectionKind::Set), Some(args)) => {
                        let elem = args
                            .first()
                            .map(Self::from_expr)
                            .unwrap_or(MirType::Unknown);
                        MirType::Set(Box::new(elem))
                    }
                    _ => MirType::Custom(name.clone()),
                }
            }
            // Meta and linear types are not involved in RC management.
            TypeKind::Meta(_) | TypeKind::Linear(_) => MirType::Unknown,
        }
    }

    /// Returns `true` if this type requires reference-count management.
    ///
    /// Mirrors the logic of [`crate::mir::rc::is_managed_type`] but operates on
    /// [`MirType`] values, avoiding any AST traversal.
    pub fn is_managed(
        &self,
        auto_copy_types: &HashSet<String>,
        type_params: &HashSet<String>,
    ) -> bool {
        match self {
            // Collections, Option, and Tuple are always RC-managed.
            MirType::Option(_)
            | MirType::List(_)
            | MirType::Array(_)
            | MirType::Map(_, _)
            | MirType::Set(_)
            | MirType::Tuple(_) => true,
            // String is RC-managed: allocated via alloc_with_rc, freed via miri_rt_string_free.
            MirType::String => true,
            // Closures are heap-allocated via alloc_with_rc and must be RC-tracked.
            MirType::Function => true,
            // Generic parameters and unknown types are never managed.
            MirType::Generic | MirType::Unknown => false,
            // Custom (user-defined) types: managed unless they are in the auto-copy
            // set, are unresolved generic placeholders, or are reserved names.
            // Also exclude unresolved collection class names (e.g. Custom("List"))
            // that appear in stdlib method signatures.
            MirType::Custom(name) => {
                name != "Self"
                    && !auto_copy_types.contains(name.as_str())
                    && !type_params.contains(name.as_str())
                    && BuiltinCollectionKind::from_name(name).is_none()
            }
            // All other types (primitives) are not managed.
            _ => false,
        }
    }

    /// Resolves a type expression node into a [`MirType`].
    ///
    /// The parser encodes type arguments inside collection type annotations
    /// (e.g. `[T]`, `[T; N]`, `{K: V}`) as [`Expression`] nodes rather than
    /// [`Type`] nodes.  This helper handles the two common forms:
    ///
    /// - `ExpressionKind::Type(ty, _)` — a fully-formed type node; recurse.
    /// - `ExpressionKind::Identifier(name, _)` — a bare name like `"String"`,
    ///   `"MyClass"`, or a generic parameter; mapped to `Custom(name)`.
    ///
    /// Any other expression form (e.g. a size literal in `[T; 4]`) maps to
    /// [`MirType::Unknown`], which is treated as non-managed.
    ///
    /// [`Expression`]: crate::ast::expression::Expression
    /// [`Type`]: crate::ast::types::Type
    fn from_expr(expr: &crate::ast::expression::Expression) -> Self {
        match &expr.node {
            crate::ast::expression::ExpressionKind::Type(ty, _) => Self::from_type_kind(&ty.kind),
            crate::ast::expression::ExpressionKind::Identifier(name, _) => {
                // A bare identifier in element-type position is a type name.
                // Store it as Custom(name) and let `is_managed` resolve it at
                // analysis time when `auto_copy_types` and `type_params` are
                // available.  This correctly handles:
                //   - User-defined class names → Custom("MyClass") → managed
                //   - Generic parameters (T, K, …) → Custom("T"), filtered by
                //     type_params in `is_managed`
                //   - Collection class names (List, Array, …) → Custom("List"),
                //     filtered by BuiltinCollectionKind in `is_managed`
                MirType::Custom(name.clone())
            }
            _ => MirType::Unknown,
        }
    }
}
