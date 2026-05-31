// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Type-level predicates and classifiers used by codegen dispatch sites.
//!
//! Pure functions only: no IR emission. Each predicate exhaustively matches
//! its target enum so adding a new variant forces this module to be revisited
//! (PRINCIPLES.md §3.5).

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::codegen::cranelift::translator::{ElementShape, FunctionTranslator};
use crate::type_checker::context::TypeDefinition;

use std::collections::HashMap;

impl<'a> FunctionTranslator<'a> {
    /// Classify an element `TypeKind` into the shape used to pick the matching
    /// runtime decref/clone helper. Folds canonical built-in collection kinds
    /// (`TypeKind::List`, `TypeKind::Array`, ...) and the post-normalization
    /// `TypeKind::Custom` form (where `BuiltinCollectionKind::from_name` is
    /// `Some`) into a single `ElementShape::Builtin` representation so
    /// dispatch sites match once.
    pub fn classify_element_shape(kind: &TypeKind) -> ElementShape<'_> {
        match kind {
            TypeKind::String => ElementShape::String,
            TypeKind::List(_) => ElementShape::Builtin(BuiltinCollectionKind::List),
            TypeKind::Array(_, _) => ElementShape::Builtin(BuiltinCollectionKind::Array),
            TypeKind::Set(_) => ElementShape::Builtin(BuiltinCollectionKind::Set),
            TypeKind::Map(_, _) => ElementShape::Builtin(BuiltinCollectionKind::Map),
            TypeKind::Custom(name, _) => match BuiltinCollectionKind::from_name(name) {
                Some(builtin) => ElementShape::Builtin(builtin),
                None => ElementShape::UserClass(name),
            },
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
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::Tuple(_)
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Function(_)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Meta(_)
            | TypeKind::Option(_)
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::Linear(_) => ElementShape::Other,
        }
    }

    /// Extracts the element expression from a Set TypeKind.
    pub(crate) fn set_elem_expr(kind: &TypeKind) -> Option<&Expression> {
        match kind {
            TypeKind::Set(e) => Some(e),
            TypeKind::Custom(name, Some(args))
                if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Set) =>
            {
                args.first()
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
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::String
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Map(_, _)
            | TypeKind::Tuple(_)
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Function(_)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Meta(_)
            | TypeKind::Option(_)
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::Linear(_) => None,
        }
    }

    /// Resolves the element `TypeKind` from a collection base type (Array,
    /// List, the post-normalization `Custom("Array"|"List", _)` form, or
    /// Tuple). Returns `None` when the base type is not a collection, when
    /// type arguments are absent, or when the element expression is not a
    /// `Type(...)` node — callers default to pointer-sized addressing.
    ///
    /// The exhaustive `TypeKind` match is deliberate: a new variant must
    /// force this site to be revisited rather than silently absorbed by a
    /// wildcard (PRINCIPLES.md §3.5, §5.4).
    pub(crate) fn resolve_collection_elem_type(base_type: &Type) -> Option<&TypeKind> {
        fn elem_kind_from_expr(expr: &Expression) -> Option<&TypeKind> {
            if let ExpressionKind::Type(ty, _) = &expr.node {
                Some(&ty.kind)
            } else {
                None
            }
        }
        match &base_type.kind {
            TypeKind::Array(elem_ty_expr, _) | TypeKind::List(elem_ty_expr) => {
                elem_kind_from_expr(elem_ty_expr)
            }
            TypeKind::Custom(name, Some(args))
                if matches!(
                    BuiltinCollectionKind::from_name(name),
                    Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
                ) =>
            {
                args.first().and_then(elem_kind_from_expr)
            }
            TypeKind::Tuple(elems) => elems.first().and_then(elem_kind_from_expr),
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
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::String
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::Map(_, _)
            | TypeKind::Set(_)
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Function(_)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Meta(_)
            | TypeKind::Option(_)
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::Linear(_) => None,
        }
    }

    /// Returns the element `Type` of a collection type (Array or List), or `None`.
    ///
    /// Unlike `resolve_collection_elem_type` which returns `&TypeKind`, this returns
    /// the full `&Type` so callers can chain through multiple projection levels.
    ///
    /// Handles the same set of base types as `resolve_collection_elem_type`:
    /// `Array(T)` / `List(T)`, the post-normalization `Custom("Array"|"List"|"Tuple", [T])`
    /// form, and homogeneous `Tuple([T, T, ...])`. For heterogeneous tuples we
    /// still return the first element's type — `Index` is only emitted for
    /// homogeneous tuples via the `Iterable` trait, and `Field` projections
    /// use `field_layout` rather than this resolver.
    pub(crate) fn resolve_collection_elem_type_as_type(base_type: &Type) -> Option<&Type> {
        fn elem_type_from_expr(expr: &Expression) -> Option<&Type> {
            if let ExpressionKind::Type(ty, _) = &expr.node {
                Some(ty.as_ref())
            } else {
                None
            }
        }
        match &base_type.kind {
            TypeKind::Array(elem_ty_expr, _) | TypeKind::List(elem_ty_expr) => {
                elem_type_from_expr(elem_ty_expr)
            }
            TypeKind::Tuple(elems) => elems.first().and_then(elem_type_from_expr),
            TypeKind::Custom(name, Some(args))
                if matches!(
                    BuiltinCollectionKind::from_name(name),
                    Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
                ) || name == crate::ast::types::TUPLE_TYPE_NAME =>
            {
                args.first().and_then(elem_type_from_expr)
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
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::String
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::Map(_, _)
            | TypeKind::Set(_)
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Function(_)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Meta(_)
            | TypeKind::Option(_)
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::Linear(_) => None,
        }
    }

    /// Returns true if the given type is a List (dynamic collection).
    pub fn is_list_type(kind: &TypeKind) -> bool {
        kind.as_builtin_collection() == Some(BuiltinCollectionKind::List)
    }

    /// Returns true if the type kind is an unsigned integer.
    pub fn is_unsigned_type_kind(kind: &TypeKind) -> bool {
        matches!(
            kind,
            TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U64 | TypeKind::U128
        )
    }

    /// Returns true if the given type is an Array, List, Map, or Set collection.
    pub fn is_collection_type(kind: &TypeKind) -> bool {
        kind.as_builtin_collection().is_some()
    }

    /// Returns true if the given type is a Map.
    pub fn is_map_type(kind: &TypeKind) -> bool {
        kind.as_builtin_collection() == Some(BuiltinCollectionKind::Map)
    }

    /// Returns true if the given type is a Set.
    pub fn is_set_type(kind: &TypeKind) -> bool {
        kind.as_builtin_collection() == Some(BuiltinCollectionKind::Set)
    }

    /// Extracts the element expression from a List or Array TypeKind.
    /// Handles both canonical variants (`TypeKind::List(e)`, `TypeKind::Array(e, _)`)
    /// and the normalised `TypeKind::Custom` form where
    /// `BuiltinCollectionKind::from_name` returns `List` or `Array`.
    pub(crate) fn collection_elem_expr(kind: &TypeKind) -> Option<&Expression> {
        match kind {
            TypeKind::List(e) | TypeKind::Array(e, _) => Some(e),
            TypeKind::Custom(name, Some(args))
                if matches!(
                    BuiltinCollectionKind::from_name(name),
                    Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array)
                ) =>
            {
                args.first()
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
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::String
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::Map(_, _)
            | TypeKind::Set(_)
            | TypeKind::Tuple(_)
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Function(_)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Meta(_)
            | TypeKind::Option(_)
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::Linear(_) => None,
        }
    }

    /// Extracts the value expression from a Map TypeKind.
    /// Handles both canonical `TypeKind::Map(_, v)` and the normalised
    /// `TypeKind::Custom` form where `BuiltinCollectionKind::from_name`
    /// returns `Map` (with `[_, v]` as generic args).
    pub(crate) fn map_val_expr(kind: &TypeKind) -> Option<&Expression> {
        match kind {
            TypeKind::Map(_, v) => Some(v),
            TypeKind::Custom(name, Some(args))
                if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Map) =>
            {
                args.get(1)
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
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::String
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Set(_)
            | TypeKind::Tuple(_)
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Function(_)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Meta(_)
            | TypeKind::Option(_)
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::Linear(_) => None,
        }
    }

    /// Returns true if a named Custom type has at least one managed field.
    ///
    /// Used to decide whether to call `__drop_TypeName` (when there are managed
    /// fields to clean up) or just `libc::free` (when all fields are primitives).
    /// Returns true if the type defines `fn drop(self)` (user-controlled teardown).
    pub(crate) fn type_has_user_drop(
        name: &str,
        type_defs: &HashMap<String, TypeDefinition>,
    ) -> bool {
        match type_defs.get(name) {
            Some(TypeDefinition::Struct(def)) => def.has_drop,
            Some(TypeDefinition::Class(def)) => def.has_drop,
            None
            | Some(TypeDefinition::Enum(_))
            | Some(TypeDefinition::Generic(_))
            | Some(TypeDefinition::Alias(_))
            | Some(TypeDefinition::Trait(_)) => false,
        }
    }

    pub(crate) fn has_managed_fields(
        name: &str,
        type_defs: &HashMap<String, TypeDefinition>,
    ) -> bool {
        match type_defs.get(name) {
            Some(TypeDefinition::Struct(def)) => def.fields.iter().any(|(_, ty, _)| {
                crate::codegen::cranelift::translator::is_field_managed(&ty.kind)
            }),
            Some(TypeDefinition::Class(def)) => def.fields.iter().any(|(_, fi)| {
                crate::codegen::cranelift::translator::is_field_managed(&fi.ty.kind)
            }),
            Some(TypeDefinition::Enum(def)) => def.variants.values().any(|fields| {
                fields
                    .iter()
                    .any(|ty| crate::codegen::cranelift::translator::is_field_managed(&ty.kind))
            }),
            None
            | Some(TypeDefinition::Generic(_))
            | Some(TypeDefinition::Alias(_))
            | Some(TypeDefinition::Trait(_)) => false,
        }
    }
}
