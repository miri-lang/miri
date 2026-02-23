// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Built-in types and functions for the type checker.
//!
//! This module defines the standard library types and functions that are
//! available in every Miri program without explicit imports.

use crate::ast::types::{TypeDeclarationKind, TypeKind};
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
        module: "std".to_string(),
    });
    types.insert("Dim3".to_string(), dim3_def);

    // GpuContext: Context available within GPU kernels
    let dim3_type = || crate::ast::factory::make_type(TypeKind::Custom("Dim3".to_string(), None));
    types.insert(
        "GpuContext".to_string(),
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
            module: "std".to_string(),
        }),
    );

    // Kernel: Opaque handle for GPU kernels
    types.insert(
        "Kernel".to_string(),
        TypeDefinition::Struct(StructDefinition {
            fields: vec![],
            generics: None,
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
            module: "std".to_string(),
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtins_initialized() {
        let (scope, types) = initialize_builtins();

        // Check that expected types exist
        // Note: String is NOT a built-in; it's defined in system/string.mi
        assert!(types.contains_key("Dim3"));
        assert!(types.contains_key("GpuContext"));
        assert!(types.contains_key("Kernel"));
        assert!(types.contains_key("Future"));

        // Verify scope is empty (print/println come from stdlib, not builtins)
        assert!(scope.is_empty());
    }

    #[test]
    fn test_dim3_has_xyz_fields() {
        let (_, types) = initialize_builtins();

        if let Some(TypeDefinition::Struct(def)) = types.get("Dim3") {
            let field_names: Vec<&str> = def.fields.iter().map(|(n, _, _)| n.as_str()).collect();
            assert_eq!(field_names, vec!["x", "y", "z"]);
        } else {
            panic!("Dim3 should be a struct");
        }
    }

    #[test]
    fn test_future_is_generic() {
        let (_, types) = initialize_builtins();

        if let Some(TypeDefinition::Struct(def)) = types.get("Future") {
            assert!(def.generics.is_some());
            let generics = def.generics.as_ref().unwrap();
            assert_eq!(generics.len(), 1);
            assert_eq!(generics[0].name, "T");
        } else {
            panic!("Future should be a struct");
        }
    }
}
