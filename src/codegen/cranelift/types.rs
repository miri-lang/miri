// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Type translation from Miri AST types to Cranelift types.

use crate::ast::types::{Type, TypeKind};
use cranelift_codegen::ir::types;
use cranelift_codegen::ir::Type as CraneliftType;

/// Translate a Miri type to a Cranelift type.
///
/// # Panics
///
/// Panics if the type cannot be represented in Cranelift (e.g., collections).
pub fn translate_type(ty: &Type, ptr_ty: CraneliftType) -> CraneliftType {
    translate_type_kind(&ty.kind, ptr_ty)
}

/// Translate a Miri TypeKind to a Cranelift type.
pub fn translate_type_kind(kind: &TypeKind, ptr_ty: CraneliftType) -> CraneliftType {
    match kind {
        TypeKind::Linear(_) => ptr_ty,
        // Integer types - signed and unsigned use the same Cranelift type
        TypeKind::I8 | TypeKind::U8 => types::I8,
        TypeKind::I16 | TypeKind::U16 => types::I16,
        TypeKind::I32 | TypeKind::U32 => types::I32,
        TypeKind::I64 | TypeKind::U64 => types::I64,
        TypeKind::I128 | TypeKind::U128 => types::I128,

        // Platform-dependent integer
        TypeKind::Int => ptr_ty,

        // Floating point types
        TypeKind::F32 => types::F32,
        TypeKind::F64 | TypeKind::Float => types::F64,

        // Boolean is represented as I8
        TypeKind::Boolean => types::I8,

        // Void type - use I8 as placeholder
        TypeKind::Void => types::I8,

        // String is a pointer
        TypeKind::String => ptr_ty,

        // Identifier type - represented as pointer-sized integer
        TypeKind::Identifier => ptr_ty,

        // Raw pointer - maps to target pointer width
        TypeKind::RawPtr => ptr_ty,

        // Collections are represented as pointers
        TypeKind::List(_) => ptr_ty,
        TypeKind::Array(_, _) => ptr_ty,
        TypeKind::Map(_, _) => ptr_ty,
        TypeKind::Set(_) => ptr_ty,
        TypeKind::Tuple(_) => ptr_ty,
        TypeKind::Result(_, _) => ptr_ty,
        TypeKind::Future(_) => ptr_ty,

        // Function types are function pointers
        TypeKind::Function(_) => ptr_ty,

        // User-defined types are pointers
        TypeKind::Custom(_, _) => ptr_ty,
        TypeKind::Generic(_, _, _) => ptr_ty,

        // Meta types should be resolved before codegen; treat as pointer-sized
        // to avoid a panic. The type checker should prevent these from reaching codegen.
        TypeKind::Meta(_) => ptr_ty,

        // Option types are pointers
        TypeKind::Option(_) => ptr_ty,

        // Error types indicate a prior compiler error; treat as pointer-sized
        // to allow graceful continuation rather than a panic.
        TypeKind::Error => ptr_ty,
    }
}
