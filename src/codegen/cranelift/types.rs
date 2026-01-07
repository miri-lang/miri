// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Type translation from Miri AST types to Cranelift types.

use crate::ast::types::{Type, TypeKind};
use cranelift_codegen::ir::types;
use cranelift_codegen::ir::Type as CraneliftType;

/// Translate a Miri type to a Cranelift type.
///
/// # Panics
///
/// Panics if the type cannot be represented in Cranelift (e.g., collections).
pub fn translate_type(ty: &Type) -> CraneliftType {
    translate_type_kind(&ty.kind)
}

/// Translate a Miri TypeKind to a Cranelift type.
pub fn translate_type_kind(kind: &TypeKind) -> CraneliftType {
    match kind {
        // Integer types - signed and unsigned use the same Cranelift type
        TypeKind::I8 | TypeKind::U8 => types::I8,
        TypeKind::I16 | TypeKind::U16 => types::I16,
        TypeKind::I32 | TypeKind::U32 => types::I32,
        TypeKind::I64 | TypeKind::U64 => types::I64,
        TypeKind::I128 | TypeKind::U128 => types::I128,

        // Platform-dependent integer
        // TODO: use I64 for now, but we should have some logic for it
        TypeKind::Int => types::I64,

        // Floating point types
        TypeKind::F32 => types::F32,
        TypeKind::F64 | TypeKind::Float => types::F64,

        // Boolean is represented as I8
        TypeKind::Boolean => types::I8,

        // Void type - use I8 as placeholder
        TypeKind::Void => types::I8,

        // String is a pointer
        TypeKind::String => types::I64,

        // Symbol type - represented as I64
        TypeKind::Symbol => types::I64,

        // Collections are represented as pointers
        TypeKind::List(_) => types::I64,
        TypeKind::Array(_, _) => types::I64,
        TypeKind::Map(_, _) => types::I64,
        TypeKind::Set(_) => types::I64,
        TypeKind::Tuple(_) => types::I64,
        TypeKind::Result(_, _) => types::I64,
        TypeKind::Future(_) => types::I64,

        // Function types are function pointers
        TypeKind::Function(_, _, _) => types::I64,

        // User-defined types are pointers
        TypeKind::Custom(_, _) => types::I64,
        TypeKind::Generic(_, _, _) => types::I64,

        // Meta types shouldn't appear at codegen time
        TypeKind::Meta(_) => panic!("Meta type should not appear during codegen"),

        // Nullable types are pointers
        TypeKind::Nullable(_) => types::I64,

        // Error types indicate a compiler bug
        TypeKind::Error => panic!("Error type should not appear during codegen"),
    }
}
