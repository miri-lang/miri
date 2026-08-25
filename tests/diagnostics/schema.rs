// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::diagnostics::json::{
    DiagnosticsEnvelope, JsonCommand, JsonDiagnostic, JsonExplanation, JsonRejectedFile,
    JsonRelated, JsonRepair, JsonTestResult, JsonTestSummary,
};
use serde_json::Value;
use std::collections::BTreeSet;

/// Load the JSON schema from docs/diagnostics-schema.json
fn load_schema() -> Value {
    let schema_path = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/diagnostics-schema.json");
    let schema_text = std::fs::read_to_string(schema_path).expect(
        "Failed to read diagnostics schema. Check that docs/diagnostics-schema.json exists.",
    );
    serde_json::from_str(&schema_text).expect("Failed to parse diagnostics schema as JSON")
}

/// Resolve a $ref to its actual schema in the root document
fn resolve_ref(ref_path: &str, root: &Value) -> Value {
    let ref_parts: Vec<&str> = ref_path.split('/').collect();
    let mut resolved = root;
    for part in ref_parts {
        if part == "#" {
            continue;
        }
        resolved = &resolved[part];
    }
    resolved.clone()
}

/// Recursively collect all property paths from a JSON schema's properties object.
/// Handles $ref definitions by resolving them from the root $defs.
fn collect_schema_properties_with_refs(
    schema: &Value,
    path: String,
    root: &Value,
) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();

    let actual_schema = if let Some(ref_path) = schema.get("$ref").and_then(|r| r.as_str()) {
        resolve_ref(ref_path, root)
    } else {
        schema.clone()
    };

    if let Some(properties) = actual_schema.get("properties").and_then(|p| p.as_object()) {
        for (key, prop_schema) in properties.iter() {
            let new_path = if path.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", path, key)
            };
            paths.insert(new_path.clone());

            // Resolve any $ref in this property
            let actual_prop =
                if let Some(ref_path) = prop_schema.get("$ref").and_then(|r| r.as_str()) {
                    resolve_ref(ref_path, root)
                } else {
                    prop_schema.clone()
                };

            // If this property has nested properties, recurse
            if actual_prop.get("properties").is_some() {
                paths.extend(collect_schema_properties_with_refs(
                    &actual_prop,
                    new_path.clone(),
                    root,
                ));
            }

            // If it's an array with object items, check those too
            if let Some(items) = actual_prop.get("items") {
                let actual_items =
                    if let Some(ref_path) = items.get("$ref").and_then(|r| r.as_str()) {
                        resolve_ref(ref_path, root)
                    } else {
                        items.clone()
                    };

                if actual_items.get("properties").is_some() {
                    paths.extend(collect_schema_properties_with_refs(
                        &actual_items,
                        new_path,
                        root,
                    ));
                }
            }
        }
    }

    paths
}

/// Recursively collect all non-null property paths from a JSON object.
/// For arrays, skip the index notation and just collect properties from the first item.
fn collect_object_properties(obj: &Value, path: String) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();

    if let Some(map) = obj.as_object() {
        for (key, value) in map.iter() {
            let new_path = if path.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", path, key)
            };

            // Only include if value is not null (the schema should document all present fields)
            if !value.is_null() {
                paths.insert(new_path.clone());
            }

            // Recurse into objects and arrays
            match value {
                Value::Object(_) => {
                    paths.extend(collect_object_properties(value, new_path));
                }
                Value::Array(arr) => {
                    // For arrays, collect properties from the first item's structure
                    // without the array index notation, to match what the schema documents.
                    if let Some(first_item @ Value::Object(_)) = arr.first() {
                        paths.extend(collect_object_properties(first_item, new_path));
                    }
                }
                _ => {}
            }
        }
    }

    paths
}

/// Build a fully-populated exemplar DiagnosticsEnvelope with every optional field set
fn build_exemplar_envelope() -> DiagnosticsEnvelope {
    DiagnosticsEnvelope {
        schema_version: 1,
        ok: false,
        command: JsonCommand::Check,
        diagnostics: vec![JsonDiagnostic {
            severity: "error".to_string(),
            code: Some("MER_TYP_001".to_string()),
            message: "Type mismatch".to_string(),
            path: Some("example.mi".to_string()),
            line: Some(10),
            column: Some(5),
            length: Some(7),
            expected: Some("Int".to_string()),
            actual: Some("String".to_string()),
            help: Some("Use an explicit type conversion".to_string()),
            fix_safety: Some("local-edit".to_string()),
            repair: Some(JsonRepair {
                id: "convert-to-int".to_string(),
                summary: "Convert the string to an integer".to_string(),
            }),
            related: vec![JsonRelated {
                severity: "note".to_string(),
                message: "Original type was declared here".to_string(),
                code: Some("MER_NAM_001".to_string()),
                path: Some("example.mi".to_string()),
                line: Some(5),
                column: Some(1),
            }],
        }],
        artifact: Some("output.bin".to_string()),
        exit_code: Some(1),
        stdout_tail: Some("Hello, world!".to_string()),
        stderr_tail: Some("Error occurred".to_string()),
        stdout_truncated: Some(false),
        stderr_truncated: Some(true),
        duration_ms: Some(1234),
        tests: Some(JsonTestSummary {
            total: 10,
            passed: 8,
            failed: 2,
            ignored: 0,
            results: vec![JsonTestResult {
                path: "tests/my_test.mi".to_string(),
                name: "test_addition".to_string(),
                outcome: "passed".to_string(),
                detail: Some("Completed in 5ms".to_string()),
            }],
            rejected_files: vec![JsonRejectedFile {
                path: "tests/unparseable.mi".to_string(),
                reason: "unparseable".to_string(),
            }],
        }),
        explanation: Some(JsonExplanation {
            code: "MER_TYP_002".to_string(),
            title: "Type Mismatch".to_string(),
            severity: "error".to_string(),
            reserved: false,
            rule: "Both sides of the assignment must have the same type.".to_string(),
            example_before: Some("let x = 1\nx = \"two\"\n".to_string()),
            example_after: Some("let x = 1\nx = 2\n".to_string()),
            reference: Some("../reference/types.md".to_string()),
        }),
    }
}

#[test]
fn test_schema_documents_all_envelope_fields() {
    let schema = load_schema();
    let exemplar = build_exemplar_envelope();
    let exemplar_json = serde_json::to_value(&exemplar).expect("Failed to serialize exemplar");

    let schema_properties = collect_schema_properties_with_refs(&schema, String::new(), &schema);
    let exemplar_properties = collect_object_properties(&exemplar_json, String::new());

    // Check that schema has all the properties present in the exemplar
    for prop in &exemplar_properties {
        assert!(
            schema_properties.contains(prop),
            "Schema is missing property that exists in exemplar: {}",
            prop
        );
    }

    // Check that all schema properties are present in the exemplar
    // (This ensures the schema doesn't document fields that don't exist in the DTO)
    for prop in &schema_properties {
        assert!(
            exemplar_properties.contains(prop),
            "Schema documents a property that doesn't exist in exemplar: {}",
            prop
        );
    }
}

#[test]
fn test_schema_is_valid_json_schema() {
    let schema = load_schema();

    // Basic checks that it's a valid JSON Schema
    assert!(schema.is_object(), "Schema must be a JSON object");
    assert!(
        schema.get("type").is_some(),
        "Schema must have a 'type' property"
    );
    assert_eq!(
        schema.get("type").and_then(|v| v.as_str()),
        Some("object"),
        "Schema root must be type 'object'"
    );
}

#[test]
fn test_exemplar_serializes_to_valid_json() {
    let exemplar = build_exemplar_envelope();
    let json_str = serde_json::to_string(&exemplar).expect("Failed to serialize exemplar");
    let parsed: Value =
        serde_json::from_str(&json_str).expect("Failed to parse serialized exemplar");

    // Verify it parses back correctly by comparing as values (not strings, since serde reorders keys)
    let exemplar_value =
        serde_json::to_value(&exemplar).expect("Failed to convert exemplar to value");
    assert_eq!(
        parsed, exemplar_value,
        "Exemplar should parse correctly as JSON"
    );
}

#[test]
fn test_schema_gate_catches_extra_fields() {
    // This test verifies that the bidirectional schema gate catches when a junk field
    // exists in an object but is not declared in the schema.

    let schema = load_schema();
    let exemplar = build_exemplar_envelope();
    let mut exemplar_json = serde_json::to_value(&exemplar).expect("Failed to serialize exemplar");

    // Simulate adding a junk field to the DiagnosticsEnvelope
    if let Some(obj) = exemplar_json.as_object_mut() {
        obj.insert(
            "junkField".to_string(),
            Value::String("should not be here".to_string()),
        );
    }

    let schema_properties = collect_schema_properties_with_refs(&schema, String::new(), &schema);
    let exemplar_properties = collect_object_properties(&exemplar_json, String::new());

    // The junk field should be detected as present in exemplar but not in schema
    // This is what the gate checks: exemplar properties must be a subset of schema properties
    let junk_detected = exemplar_properties.contains("junkField");
    assert!(
        junk_detected,
        "Test setup failed: junkField should be present in modified exemplar"
    );

    // Verify the gate catches this mismatch: if a property exists in exemplar but not
    // in schema, the gate's first check should fail
    let junk_in_schema = schema_properties.contains("junkField");
    assert!(
        !junk_in_schema,
        "Gate must catch: junkField is in exemplar but not in schema"
    );

    // Verify that the gate's first assertion would actually fail
    // (schema properties must contain all exemplar properties)
    let gate_would_fail = !exemplar_properties.is_subset(&schema_properties);
    assert!(
        gate_would_fail,
        "The gate's bidirectional check should fail because exemplar has extra properties not in schema"
    );
}
