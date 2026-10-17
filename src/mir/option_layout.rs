// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The layout authority for optional values.
//!
//! An optional whose payload is itself a pointer to a reference-counted
//! allocation can spell its absence as a null payload pointer, with no box of
//! its own. Every other optional layer — a scalar or inline-vector payload, or
//! an outer layer of a nested optional — needs a box, so that the layer's
//! absence and the inner value's absence stay distinguishable.
//!
//! [`OptionalLayers`] is the one place that decides how many layers of a
//! (possibly nested) optional are boxed, and [`payload_is_nullable_pointer`]
//! the one place that decides which payloads are such pointers. Every site that
//! builds, reads or releases an optional asks these, so they agree.

use crate::ast::expression::Expression;
use crate::ast::types::{BuiltinCollectionKind, TypeKind};
use crate::mir::rc::is_field_managed;
use crate::type_checker::context::TypeDefinition;
use std::collections::HashMap;

/// How a possibly nested optional type is laid out.
///
/// For `Node??` with `Node` a class, the innermost layer is the nullable
/// `Node` pointer and the outer layer is boxed: `boxed` is 1,
/// `innermost_nullable_pointer` is true and `value` is `Node`. For `int??`
/// both layers are boxed. A type that is not optional has no layers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OptionalLayers<'t> {
    /// The number of optional layers that hold their payload in a box.
    pub boxed: usize,
    /// Whether the innermost optional layer is the payload pointer itself,
    /// null when absent.
    pub innermost_nullable_pointer: bool,
    /// The innermost non-optional kind.
    pub value: &'t TypeKind,
}

impl<'t> OptionalLayers<'t> {
    /// The layout of `kind`, where `definitions` is the program's type table.
    ///
    /// Only `TypeKind::Option` layers are peeled: an alias to an optional is
    /// the innermost value, laid out by its own layers when it is held.
    pub fn of(kind: &'t TypeKind, definitions: &HashMap<String, TypeDefinition>) -> Self {
        let mut layers = 0;
        let mut value = kind;
        while let TypeKind::Option(inner) = value {
            layers += 1;
            value = &inner.kind;
        }
        let innermost_nullable_pointer =
            layers > 0 && payload_is_nullable_pointer(value, definitions);
        Self {
            boxed: layers - usize::from(innermost_nullable_pointer),
            innermost_nullable_pointer,
            value,
        }
    }
}

/// Whether an optional holding a `kind` payload can be that payload's pointer,
/// null when absent.
///
/// True for a string, a built-in collection at its type arguments, a tuple, a
/// function value, and a name the type table defines as a class, struct, enum
/// or trait (through any aliases). False for every value held by value —
/// scalars, aliases to them, inline vectors and atomics — for an optional
/// (whose own absence must stay distinct from the outer one), for names that
/// denote no allocation (generic parameters, placeholders, a collection named
/// without arguments inside its own body), and for kinds no optional payload
/// is a reference-counted pointer of.
///
/// A generic alias is judged by its template as written, so one that reaches
/// a pointer only through its arguments answers false and stays boxed.
pub fn payload_is_nullable_pointer(
    kind: &TypeKind,
    definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    match kind {
        TypeKind::String
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Tuple(_)
        | TypeKind::Function(_) => true,
        TypeKind::Custom(name, args) => {
            is_field_managed(kind) && names_allocation(name, args.as_deref(), definitions)
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
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Generic(_, _, _)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Linear(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::OneOf(_) => false,
    }
}

/// Whether `name<args>` names a reference-counted allocation.
///
/// A built-in collection does only at its type arguments: named bare, it is
/// the unresolved self-reference inside the collection's own body. Any other
/// name is decided by its definition, an alias by the type it spells.
fn names_allocation(
    name: &str,
    args: Option<&[Expression]>,
    definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    if BuiltinCollectionKind::from_name(name).is_some() {
        return args.is_some();
    }
    match definitions.get(name) {
        Some(
            TypeDefinition::Class(_)
            | TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Trait(_),
        ) => true,
        Some(TypeDefinition::Alias(alias)) => {
            payload_is_nullable_pointer(&alias.template.kind, definitions)
        }
        Some(TypeDefinition::Generic(_)) | None => false,
    }
}
