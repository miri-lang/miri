// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Type-level predicates and classifiers used by codegen dispatch sites.
//!
//! Pure functions only: no IR emission. Each predicate exhaustively matches
//! its target enum so adding a new variant forces this module to be revisited.

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
            TypeKind::Custom(name, _) => {
                // Vector types (Vec2/3/4) and `Atomic<scalar>` are value types
                // stored inline — they carry no per-element drop/clone callback.
                if crate::ast::types::vec_type_dim(kind).is_some()
                    || name == crate::ast::types::ATOMIC_TYPE_NAME
                {
                    return ElementShape::Other;
                }
                match BuiltinCollectionKind::from_name(name) {
                    Some(builtin) => ElementShape::Builtin(builtin),
                    None => ElementShape::UserClass(name),
                }
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
            | TypeKind::F16
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

    /// The element kind that makes a set match its elements, or a map its keys,
    /// by their raw bytes. Mirrors the runtime's `element_identity::BY_BYTES`,
    /// and is what every container starts at.
    pub(crate) const BYTES_ELEMENT_KIND: i64 = 0;

    /// The element kind that makes a set match its string elements, or a map
    /// its string keys, by content. Mirrors the runtime's
    /// `element_identity::BY_STRING_CONTENT`.
    pub(crate) const STRING_CONTENT_ELEMENT_KIND: i64 = 1;

    /// Bits the element kind reserves for the rule that settles a value.
    /// Mirrors the runtime's `element_identity`, which reads the word back.
    const ELEMENT_RULE_BITS: u32 = 8;

    /// Bits the element kind reserves for the number of optionals wrapping the
    /// element, immediately above the rule.
    const OPTIONAL_DEPTH_BITS: u32 = 8;

    /// How many optionals the element kind can say an element is wrapped in.
    /// Mirrors the runtime's `element_identity::MAX_OPTIONAL_DEPTH`.
    const MAX_OPTIONAL_DEPTH: usize = (1 << Self::OPTIONAL_DEPTH_BITS) - 1;

    /// The widest boxed value the element kind can state, which is the widest
    /// scalar the backend has: the field holding it is the rest of the word,
    /// but a size beyond this means the caller measured something other than a
    /// boxed value.
    const MAX_OPTIONAL_VALUE_SIZE: u32 = 16;

    /// The element kind registering `rule` for the value reached by opening
    /// `depth` optionals, each box holding `value_size` bytes.
    ///
    /// `None` when the word cannot say it: no optional to open, a depth past
    /// what the field holds, or a value of an unstatable width. Declining
    /// leaves the container matching such an element by its bytes, which is
    /// what it did before an optional could be described at all — a truncated
    /// depth would instead open the wrong number of boxes and read a pointer
    /// as a value.
    pub(crate) fn optional_element_kind(rule: i64, depth: usize, value_size: u32) -> Option<i64> {
        if depth == 0 || depth > Self::MAX_OPTIONAL_DEPTH {
            return None;
        }
        if value_size == 0 || value_size > Self::MAX_OPTIONAL_VALUE_SIZE {
            return None;
        }
        let size = i64::from(value_size) << (Self::ELEMENT_RULE_BITS + Self::OPTIONAL_DEPTH_BITS);
        Some(rule | (depth as i64) << Self::ELEMENT_RULE_BITS | size)
    }

    /// How many optionals wrap `kind`, and the type they wrap.
    pub(crate) fn peel_optionals(kind: &TypeKind) -> (usize, &TypeKind) {
        let mut depth = 0;
        let mut inner = kind;
        while let TypeKind::Option(payload) = inner {
            depth += 1;
            inner = &payload.kind;
        }
        (depth, inner)
    }

    /// True when an element of `kind` is the same element as another exactly
    /// when their bytes agree: a scalar, whose bytes are its value.
    ///
    /// A float is one of them, on the same terms a container of bare floats
    /// already matches on: its bytes settle it, so a negative zero is not the
    /// zero `==` says it equals and a NaN is the NaN `==` says it is not. The
    /// wrapped element answers as the bare one does, which is what makes the
    /// two containers agree with each other.
    ///
    /// TODO: settling those two cases needs a float rule of its own beside the
    /// byte and content rules, and first a decision on whether membership
    /// follows `==` or identity of value.
    pub(crate) fn is_matched_by_bytes(kind: &TypeKind) -> bool {
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
            | TypeKind::F16
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr => true,
            TypeKind::String
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Set(_)
            | TypeKind::Map(_, _)
            | TypeKind::Tuple(_)
            | TypeKind::Custom(_, _)
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Function(_)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Meta(_)
            | TypeKind::Option(_)
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::Linear(_) => false,
        }
    }

    /// The order kind that makes a list or array sort its elements as unsigned
    /// integers. Mirrors the runtime's `element_order::BY_UNSIGNED_VALUE`; the
    /// default `0` reads an element's bytes as a signed value.
    pub(crate) const UNSIGNED_VALUE_ORDER_KIND: i64 = 1;

    /// The order kind that makes a list or array sort its elements as floats.
    /// Mirrors the runtime's `element_order::BY_FLOAT_VALUE`.
    pub(crate) const FLOAT_VALUE_ORDER_KIND: i64 = 2;

    /// The order kind a list or array of `elem_kind` must register so its bytes
    /// sort by value, or `None` when the runtime's signed default already does
    /// (signed integers, booleans) or the element carries no value in its bytes.
    pub(crate) fn element_order_kind(elem_kind: &TypeKind) -> Option<i64> {
        match elem_kind {
            TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U64 | TypeKind::U128 => {
                Some(Self::UNSIGNED_VALUE_ORDER_KIND)
            }
            TypeKind::Float | TypeKind::F16 | TypeKind::F32 | TypeKind::F64 => {
                Some(Self::FLOAT_VALUE_ORDER_KIND)
            }
            TypeKind::Int
            | TypeKind::I8
            | TypeKind::I16
            | TypeKind::I32
            | TypeKind::I64
            | TypeKind::I128
            | TypeKind::Boolean
            | TypeKind::String
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Set(_)
            | TypeKind::Map(_, _)
            | TypeKind::Tuple(_)
            | TypeKind::Custom(_, _)
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

    /// Extracts the key and value expressions from a Map TypeKind.
    ///
    /// Mirrors [`Self::set_elem_expr`]: a map written as a type literal carries
    /// its arguments in `TypeKind::Map`, while one that reached codegen through
    /// a generic instantiation carries them as `Custom` type arguments.
    pub(crate) fn map_key_value_exprs(kind: &TypeKind) -> Option<(&Expression, &Expression)> {
        match kind {
            TypeKind::Map(k, v) => Some((k, v)),
            TypeKind::Custom(name, Some(args))
                if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Map) =>
            {
                Some((args.first()?, args.get(1)?))
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

    /// Core implementation: resolves the element `TypeKind` from a collection
    /// kind (Array, List, Tuple, or the post-normalization `Custom` forms).
    /// Returns `None` when the base kind is not a collection, when type
    /// arguments are absent, or when the element expression is not a
    /// `Type(...)` node.
    ///
    /// This is the canonical home for all collection element type resolution.
    /// Call this when you need just the TypeKind; wrap it as needed for
    /// &TypeKind or &Type returns.
    pub(crate) fn resolve_collection_elem_type_kind_impl(kind: &TypeKind) -> Option<&TypeKind> {
        fn elem_kind_from_expr(expr: &Expression) -> Option<&TypeKind> {
            if let ExpressionKind::Type(ty, _) = &expr.node {
                Some(&ty.kind)
            } else {
                None
            }
        }
        match kind {
            TypeKind::Array(elem_ty_expr, _) | TypeKind::List(elem_ty_expr) => {
                elem_kind_from_expr(elem_ty_expr)
            }
            TypeKind::Tuple(elems) => elems.first().and_then(elem_kind_from_expr),
            TypeKind::Custom(name, Some(args))
                if matches!(
                    BuiltinCollectionKind::from_name(name),
                    Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
                ) || name == crate::ast::types::TUPLE_TYPE_NAME =>
            {
                args.first().and_then(elem_kind_from_expr)
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

    /// Resolves the element `TypeKind` from a collection base type (Array,
    /// List, the post-normalization `Custom("Array"|"List"|"Tuple", _)` form, or
    /// Tuple). Returns `None` when the base type is not a collection, when
    /// type arguments are absent, or when the element expression is not a
    /// `Type(...)` node — callers default to pointer-sized addressing.
    ///
    /// The exhaustive `TypeKind` match is deliberate: a new variant must
    /// force this site to be revisited rather than silently absorbed by a
    /// wildcard pattern.
    pub(crate) fn resolve_collection_elem_type(base_type: &Type) -> Option<&TypeKind> {
        Self::resolve_collection_elem_type_kind_impl(&base_type.kind)
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
        // Use the core impl to resolve the TypeKind first, then extract the full Type
        // from the matching expression to preserve the full Type context.
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
            | TypeKind::F16
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

    /// Returns true if the type kind is any integer (signed or unsigned).
    pub fn is_integer_kind(kind: &TypeKind) -> bool {
        matches!(
            kind,
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
            | TypeKind::F16
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
            | TypeKind::F16
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
    /// Used to decide whether to call `miri.TypeName.$drop` (when there are managed
    /// fields to clean up) or just `libc::free` (when all fields are primitives).
    /// Returns true if releasing the type runs a `fn drop(self)` hook, declared
    /// on the type itself or inherited from a base class.
    pub(crate) fn type_has_user_drop(
        name: &str,
        type_defs: &HashMap<String, TypeDefinition>,
    ) -> bool {
        crate::type_checker::utils::has_drop_hook(name, type_defs)
    }

    /// Whether releasing an instance of `name` has a reference to give up.
    ///
    /// A class is asked about every field it stores, the ones it inherits
    /// included: those belong to the instance just as much as its own, and a
    /// class that declares none of its own still has to release what its parent
    /// declared. An inherited field is read at the type the `extends` chain
    /// pins it to, so `class Child extends Base<String>` sees a string where
    /// the parent wrote a parameter.
    pub(crate) fn has_managed_fields(
        name: &str,
        type_defs: &HashMap<String, TypeDefinition>,
    ) -> bool {
        match type_defs.get(name) {
            Some(TypeDefinition::Struct(def)) => def
                .fields
                .iter()
                .any(|(_, ty, _)| crate::mir::rc::is_word_slot_managed(&ty.kind)),
            Some(TypeDefinition::Class(_)) => {
                crate::mir::lowering::inherited_instantiation::instantiated_field_types(
                    type_defs,
                    name,
                    &[],
                )
                .iter()
                .any(|ty| crate::mir::rc::is_word_slot_managed(&ty.kind))
            }
            Some(TypeDefinition::Enum(def)) => def.variants.values().any(|fields| {
                fields
                    .iter()
                    .any(|ty| crate::mir::rc::is_word_slot_managed(&ty.kind))
            }),
            None
            | Some(TypeDefinition::Generic(_))
            | Some(TypeDefinition::Alias(_))
            | Some(TypeDefinition::Trait(_)) => false,
        }
    }
}
