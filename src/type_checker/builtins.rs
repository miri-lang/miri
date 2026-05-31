// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Built-in types and functions for the type checker.
//!
//! This module defines the standard library types and functions that are
//! available in every Miri program without explicit imports.

use crate::ast::types::{
    TypeDeclarationKind, TypeKind, DIM3_TYPE_NAME, GPU_CONTEXT_TYPE_NAME, KERNEL_TYPE_NAME,
};
use crate::ast::MemberVisibility;
use std::collections::HashMap;

use super::context::{GenericDefinition, StructDefinition, TypeDefinition};

/// Initializes built-in types and functions.
///
/// Returns a tuple of:
/// - Global scope containing built-in functions (e.g., `print`)
/// - Global type definitions containing built-in types (e.g., `String`, `Dim3`)
pub fn initialize_builtins() -> (
    HashMap<String, super::context::SymbolInfo>,
    HashMap<String, TypeDefinition>,
) {
    let mut type_definitions = HashMap::new();
    let global_scope = HashMap::new();

    register_primitive_types(&mut type_definitions);
    register_gpu_types(&mut type_definitions);
    register_async_types(&mut type_definitions);

    (global_scope, type_definitions)
}

/// Registers primitive built-in types.
///
/// Note: `String` is intentionally NOT registered here because it is fully defined
/// in the standard library (`system/string.mi`). Registering it as a built-in would
/// cause a "Type 'String' is already defined" error when the stdlib is imported.
fn register_primitive_types(_types: &mut HashMap<String, TypeDefinition>) {
    // Currently empty — all primitive types are either represented as TypeKind variants
    // (Int, Float, Bool, String) or defined in the standard library.
}

/// Registers GPU-related built-in types (`Dim3`, `GpuContext`, `Kernel`).
fn register_gpu_types(types: &mut HashMap<String, TypeDefinition>) {
    let int_type = || crate::ast::factory::make_type(TypeKind::Int);

    // Dim3: 3D dimension type for GPU operations
    let dim3_def = TypeDefinition::Struct(StructDefinition {
        fields: vec![
            ("x".to_string(), int_type(), MemberVisibility::Public),
            ("y".to_string(), int_type(), MemberVisibility::Public),
            ("z".to_string(), int_type(), MemberVisibility::Public),
        ],
        generics: None,
        has_drop: false,
        module: "std".to_string(),
    });
    types.insert(DIM3_TYPE_NAME.to_string(), dim3_def);

    // GpuContext: Context available within GPU kernels
    let dim3_type =
        || crate::ast::factory::make_type(TypeKind::Custom(DIM3_TYPE_NAME.to_string(), None));
    types.insert(
        GPU_CONTEXT_TYPE_NAME.to_string(),
        TypeDefinition::Struct(StructDefinition {
            fields: vec![
                (
                    "thread_idx".to_string(),
                    dim3_type(),
                    MemberVisibility::Public,
                ),
                (
                    "block_idx".to_string(),
                    dim3_type(),
                    MemberVisibility::Public,
                ),
                (
                    "block_dim".to_string(),
                    dim3_type(),
                    MemberVisibility::Public,
                ),
                (
                    "grid_dim".to_string(),
                    dim3_type(),
                    MemberVisibility::Public,
                ),
            ],
            generics: None,
            has_drop: false,
            module: "std".to_string(),
        }),
    );

    // Kernel: Opaque handle for GPU kernels
    types.insert(
        KERNEL_TYPE_NAME.to_string(),
        TypeDefinition::Struct(StructDefinition {
            fields: vec![],
            generics: None,
            has_drop: false,
            module: "std".to_string(),
        }),
    );
}

/// Registers async-related built-in types (`Future<T>`).
fn register_async_types(types: &mut HashMap<String, TypeDefinition>) {
    // Future<T>: Represents an async computation
    types.insert(
        "Future".to_string(),
        TypeDefinition::Struct(StructDefinition {
            fields: vec![], // Opaque type
            generics: Some(vec![GenericDefinition {
                name: "T".to_string(),
                constraint: None,
                kind: TypeDeclarationKind::None,
            }]),
            has_drop: false,
            module: "std".to_string(),
        }),
    );
}
