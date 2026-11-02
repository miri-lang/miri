// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Type-level predicates and classifiers used by codegen dispatch sites.
//!
//! Pure functions only: no IR emission. Each predicate exhaustively matches
//! its target enum so adding a new variant forces this module to be revisited.

use crate::abi::layout as abi;
use crate::ast::type_arguments::argument_type;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::codegen::cranelift::translator::{ElementShape, FunctionTranslator, TypeCtx};
use crate::error::CodegenError;
use crate::mir::option_layout::OptionalLayers;
use crate::mir::type_facts::TypeDefinition;

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
                // `Atomic<scalar>` is stored as its scalar — it carries no
                // per-element drop/clone callback.
                if name == crate::ast::types::ATOMIC_TYPE_NAME {
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
            | TypeKind::OneOf(_)
            | TypeKind::Meta(_)
            | TypeKind::Option(_)
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::Linear(_) => ElementShape::Other,
        }
    }

    /// The element type of a set, or `None` when `kind` is no set or its
    /// element names no type.
    pub(crate) fn set_element_type(kind: &TypeKind) -> Option<&Type> {
        match kind {
            TypeKind::Set(e) => argument_type(e),
            TypeKind::Custom(name, Some(args))
                if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Set) =>
            {
                args.first().and_then(argument_type)
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
            | TypeKind::OneOf(_)
            | TypeKind::Meta(_)
            | TypeKind::Option(_)
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::Linear(_) => None,
        }
    }

    /// The widest boxed value the element kind can state, which is the widest
    /// value held as its bytes: the field holding it is the rest of the word,
    /// but a size beyond this means the caller measured something other than a
    /// boxed value.
    const MAX_OPTIONAL_VALUE_SIZE: u32 = crate::ast::types::AUTO_COPY_MAX_SIZE as u32;

    /// `kind`, with the element's callbacks handed the address its value
    /// resolves to, as an element held inline as its bytes needs.
    pub(crate) fn by_slot_address(kind: i64) -> i64 {
        abi::by_slot_address(kind as usize) as i64
    }

    /// The element kind registering `rule` for the value reached by opening
    /// the `layers` of an optional element, the last box holding `value_size`
    /// bytes.
    ///
    /// `None` when the word cannot say it: no optional to open, a boxed depth
    /// past what the field holds, or a value of an unstatable width. Declining
    /// leaves the container matching such an element by its bytes, which is
    /// what it did before an optional could be described at all — a truncated
    /// depth would instead open the wrong number of boxes and read a pointer
    /// as a value.
    pub fn optional_element_kind(
        rule: i64,
        layers: &OptionalLayers<'_>,
        value_size: u32,
    ) -> Option<i64> {
        // An inline layer has no box to open: an optional held as its bytes
        // is matched through its own callbacks, not described here.
        if !layers.is_optional() || layers.inline > 0 || layers.boxed > abi::MAX_OPTIONAL_DEPTH {
            return None;
        }
        if value_size == 0 || value_size > Self::MAX_OPTIONAL_VALUE_SIZE {
            return None;
        }
        let kind = abi::through_optionals(
            rule as usize,
            layers.boxed,
            value_size as usize,
            layers.innermost_nullable_pointer,
        );
        Some(kind as i64)
    }

    /// Whether a value of `kind` is an optional held as its payload's own
    /// pointer, null when absent, rather than as a box. The layout authority
    /// decides; every codegen site that builds, reads, moves or releases an
    /// optional asks through here.
    ///
    /// An optional whose payload the backend cannot lay out is reported rather
    /// than boxed by default, since a guess hands a box to a reader expecting
    /// the bare pointer or the reverse: one at the error type, whose type was
    /// never settled, and one at a type parameter the code being translated
    /// does not leave open, which missed its substitution.
    pub(crate) fn nullable_pointer_layout(
        kind: &TypeKind,
        type_ctx: &TypeCtx,
    ) -> Result<bool, CodegenError> {
        let layers = OptionalLayers::of(kind, type_ctx.layout_scope());
        if !layers.is_optional() || layers.inline > 0 {
            return Ok(false);
        }
        if let Some(reason) = Self::undecidable_payload(&layers.value, type_ctx) {
            return Err(CodegenError::Internal(format!(
                "the optional `{kind:?}` holds {reason}, so whether it is a box or its \
                 payload's own pointer cannot be decided"
            )));
        }
        Ok(layers.boxed == 0 && layers.innermost_nullable_pointer)
    }

    /// Why an optional of `value` cannot be laid out, or `None` when it can.
    fn undecidable_payload(value: &TypeKind, type_ctx: &TypeCtx) -> Option<String> {
        if matches!(value, TypeKind::Error) {
            return Some("the error type".to_string());
        }
        let parameter = Self::type_parameter_name(value, type_ctx.facts.definitions())?;
        (!type_ctx.open_params.contains(parameter)).then(|| {
            format!(
                "the type parameter `{parameter}`, which the code being translated leaves \
                 no room for"
            )
        })
    }

    /// The name of the type parameter `kind` stands for, in either spelling:
    /// a generic placeholder, or a bare name the type table defines as one.
    fn type_parameter_name<'k>(
        kind: &'k TypeKind,
        definitions: &HashMap<String, TypeDefinition>,
    ) -> Option<&'k str> {
        if let TypeKind::Generic(name, _, _) = kind {
            return Some(name);
        }
        let TypeKind::Custom(name, None) = kind else {
            return None;
        };
        matches!(definitions.get(name), Some(TypeDefinition::Generic(_))).then_some(name)
    }

    /// The order kind a list or array of `elem_kind` must register so its bytes
    /// sort by value, or `None` when the runtime's signed default already does
    /// (signed integers, booleans) or the element carries no value in its bytes.
    pub(crate) fn element_order_kind(elem_kind: &TypeKind) -> Option<i64> {
        match elem_kind {
            TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U64 | TypeKind::U128 => {
                Some(abi::BY_UNSIGNED_VALUE as i64)
            }
            TypeKind::Float | TypeKind::F16 | TypeKind::F32 | TypeKind::F64 => {
                Some(abi::BY_FLOAT_VALUE as i64)
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
            | TypeKind::OneOf(_)
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

    /// The key and value types of a map, each `None` when it names no type;
    /// `None` when `kind` is no map spelled with both arguments.
    ///
    /// Mirrors [`Self::set_element_type`]: a map written as a type literal
    /// carries its arguments in `TypeKind::Map`, while one that reached codegen
    /// through a generic instantiation carries them as `Custom` type arguments.
    pub(crate) fn map_key_value_types(kind: &TypeKind) -> Option<(Option<&Type>, Option<&Type>)> {
        match kind {
            TypeKind::Map(k, v) => Some((argument_type(k), argument_type(v))),
            TypeKind::Custom(name, Some(args))
                if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Map) =>
            {
                Some((argument_type(args.first()?), argument_type(args.get(1)?)))
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
            | TypeKind::OneOf(_)
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
        let elem_kind_from_expr = |expr| argument_type(expr).map(|ty| &ty.kind);
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
            | TypeKind::OneOf(_)
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
        let elem_type_from_expr = argument_type;
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
            | TypeKind::OneOf(_)
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

    /// The element type of a list or array, or `None` when `kind` is neither
    /// or its element names no type. Handles both canonical variants
    /// (`TypeKind::List(e)`, `TypeKind::Array(e, _)`) and the normalised
    /// `TypeKind::Custom` form where `BuiltinCollectionKind::from_name` returns
    /// `List` or `Array`.
    pub(crate) fn collection_element_type(kind: &TypeKind) -> Option<&Type> {
        match kind {
            TypeKind::List(e) | TypeKind::Array(e, _) => argument_type(e),
            TypeKind::Custom(name, Some(args))
                if matches!(
                    BuiltinCollectionKind::from_name(name),
                    Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array)
                ) =>
            {
                args.first().and_then(argument_type)
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
            | TypeKind::OneOf(_)
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
}
