// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::type_checker::builtins::initialize_builtins;
use miri::type_checker::context::TypeDefinition;

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
        if let Some(generics) = def.generics.as_ref() {
            assert_eq!(generics.len(), 1);
            assert_eq!(generics[0].name, "T");
        }
    } else {
        panic!("Future should be a struct");
    }
}
