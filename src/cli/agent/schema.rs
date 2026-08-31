// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! JSON-RPC method schemas for `miri agent`.
//!
//! This module defines the shape of each method's parameters as a typed table,
//! allowing the schema to be the single source of truth for both JSON-Schema
//! generation and human-readable error messages. Documentation examples in
//! `docs/agent-protocol.md` are hand-written (not generated from this table)
//! but must be kept in sync with the schema they describe.

use serde_json::{json, Value};
use std::collections::BTreeMap;

/// What a parameter accepts.
#[derive(Debug)]
pub enum Shape {
    /// A string value.
    Text,
    /// A boolean flag.
    Flag,
    /// An enumerated string value.
    Choice(&'static [&'static str]),
    /// An array of operation objects.
    Operations,
}

impl Shape {
    /// Render this shape as a JSON-Schema fragment (the `type`, `enum`, etc.).
    fn to_json_schema(&self) -> Value {
        match self {
            Shape::Text => json!({ "type": "string" }),
            Shape::Flag => json!({ "type": "boolean" }),
            Shape::Choice(values) => json!({
                "type": "string",
                "enum": values
            }),
            Shape::Operations => json!({
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["function"],
                    "properties": {
                        "function": { "type": "string" },
                        "old": { "type": "string" },
                        "new": { "type": "string" },
                        "body": { "type": "string" },
                        "insert": { "type": "string" },
                        "after": { "type": "string" }
                    },
                    "oneOf": [
                        {
                            "required": ["function", "old", "new"],
                            "not": {
                                "anyOf": [
                                    { "required": ["body"] },
                                    { "required": ["insert"] }
                                ]
                            }
                        },
                        {
                            "required": ["function", "body"],
                            "not": {
                                "anyOf": [
                                    { "required": ["old"] },
                                    { "required": ["new"] },
                                    { "required": ["insert"] }
                                ]
                            }
                        },
                        {
                            "required": ["function", "insert"],
                            "not": {
                                "anyOf": [
                                    { "required": ["old"] },
                                    { "required": ["new"] },
                                    { "required": ["body"] }
                                ]
                            }
                        }
                    ]
                }
            }),
        }
    }

    /// Render a human-readable type description.
    fn display(&self) -> &'static str {
        match self {
            Shape::Text => "string",
            Shape::Flag => "boolean",
            Shape::Choice(_) => "string",
            Shape::Operations => "array",
        }
    }
}

/// One parameter of a method.
#[derive(Debug)]
pub struct Param {
    /// The parameter name.
    pub name: &'static str,
    /// What values it accepts.
    pub shape: Shape,
    /// Whether it must be present.
    pub required: bool,
    /// The parameter this one is meaningless without, if any.
    pub requires: Option<&'static str>,
    /// A brief description.
    pub description: &'static str,
}

impl Param {
    /// Render this parameter in human-readable form for error messages.
    ///
    /// Returns a string like "path required string" or "around optional string".
    fn display(&self) -> String {
        let mut parts = vec![self.name.to_string()];

        if self.required {
            parts.push("required".to_string());
        } else {
            parts.push("optional".to_string());
        }

        parts.push(self.shape.display().to_string());

        if let Shape::Choice(values) = self.shape {
            let choices = values
                .iter()
                .map(|v| format!("`{}`", v))
                .collect::<Vec<_>>()
                .join("|");
            parts.push(choices);
        }

        parts.join(" ")
    }
}

/// The schema of one served method.
pub struct MethodSchema {
    /// The method name.
    pub method: &'static str,
    /// Its parameters.
    pub params: &'static [Param],
}

impl MethodSchema {
    /// Render this method's schema as a JSON-Schema fragment.
    fn to_json_schema(&self) -> Value {
        let mut properties = BTreeMap::new();
        let mut required = Vec::new();
        let mut dependent_required = BTreeMap::new();

        for param in self.params {
            let mut schema = param.shape.to_json_schema();

            // Add description to the schema
            if let Value::Object(ref mut obj) = schema {
                obj.insert(
                    "description".to_string(),
                    Value::String(param.description.to_string()),
                );
            }

            properties.insert(param.name.to_string(), schema);

            if param.required {
                required.push(Value::String(param.name.to_string()));
            }

            if let Some(depends_on) = param.requires {
                dependent_required
                    .entry(param.name.to_string())
                    .or_insert_with(Vec::new)
                    .push(Value::String(depends_on.to_string()));
            }
        }

        let mut schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": properties
        });

        if !required.is_empty() {
            schema["required"] = json!(required);
        }

        if !dependent_required.is_empty() {
            schema["dependentRequired"] = json!(dependent_required);
        }

        schema
    }
}

/// Schema definitions for all served methods.
pub const METHODS: &[MethodSchema] = &[
    MethodSchema {
        method: "initialize",
        params: &[],
    },
    MethodSchema {
        method: "check",
        params: &[
            Param {
                name: "path",
                shape: Shape::Text,
                required: true,
                requires: None,
                description: "path to the source file to check",
            },
            Param {
                name: "verifyMir",
                shape: Shape::Flag,
                required: false,
                requires: None,
                description: "whether to verify MIR validity",
            },
        ],
    },
    MethodSchema {
        method: "explain",
        params: &[Param {
            name: "code",
            shape: Shape::Text,
            required: true,
            requires: None,
            description: "the diagnostic code to explain",
        }],
    },
    MethodSchema {
        method: "fixPlan",
        params: &[Param {
            name: "path",
            shape: Shape::Text,
            required: true,
            requires: None,
            description: "path to the source file",
        }],
    },
    MethodSchema {
        method: "fixApply",
        params: &[
            Param {
                name: "path",
                shape: Shape::Text,
                required: true,
                requires: None,
                description: "path to the source file",
            },
            Param {
                name: "allowRisky",
                shape: Shape::Flag,
                required: false,
                requires: None,
                description: "whether to apply repairs classified as risky",
            },
        ],
    },
    MethodSchema {
        method: "view",
        params: &[
            Param {
                name: "path",
                shape: Shape::Text,
                required: true,
                requires: None,
                description: "path to the source file",
            },
            Param {
                name: "fn",
                shape: Shape::Text,
                required: false,
                requires: None,
                description: "function name to read",
            },
            Param {
                name: "around",
                shape: Shape::Text,
                required: false,
                requires: Some("fn"),
                description: "text to narrow the function view by",
            },
        ],
    },
    MethodSchema {
        method: "patch",
        params: &[
            Param {
                name: "path",
                shape: Shape::Text,
                required: true,
                requires: None,
                description: "path to the source file to edit",
            },
            Param {
                name: "operations",
                shape: Shape::Operations,
                required: true,
                requires: None,
                description: "edits to apply",
            },
            Param {
                name: "mode",
                shape: Shape::Choice(&["apply", "checkOnly", "dryRun"]),
                required: false,
                requires: None,
                description: "whether to apply, check only, or dry-run the edits",
            },
            Param {
                name: "expectSha",
                shape: Shape::Text,
                required: false,
                requires: None,
                description: "expected SHA-256 hash of the file before edits",
            },
        ],
    },
    MethodSchema {
        method: "skillsGet",
        params: &[Param {
            name: "name",
            shape: Shape::Text,
            required: false,
            requires: None,
            description: "name of a specific skill to retrieve",
        }],
    },
];

/// Generate JSON-Schema fragments for all methods.
///
/// Returns a map from method name to the JSON-Schema fragment that describes
/// the method's parameters.
pub fn fragments() -> BTreeMap<String, Value> {
    let mut result = BTreeMap::new();
    for method in METHODS {
        result.insert(method.method.to_string(), method.to_json_schema());
    }
    result
}

/// Generate a human-readable parameter list for a method.
///
/// Returns a string like "path required string, fn optional string".
pub fn accepted(method: &str) -> Option<String> {
    METHODS.iter().find(|m| m.method == method).map(|method| {
        method
            .params
            .iter()
            .map(|p| p.display())
            .collect::<Vec<_>>()
            .join(", ")
    })
}
