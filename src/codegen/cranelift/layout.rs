// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Layout computation for aggregate types.
//!
//! Provides helpers to compute byte offsets and sizes for fields within
//! structs, tuples, and enums during Cranelift code generation.

use crate::ast::expression::ExpressionKind;
use crate::ast::types::TypeKind;
use crate::codegen::cranelift::types::translate_type_kind;
use crate::type_checker::context::TypeDefinition;
use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::Type as CraneliftType;
use std::collections::HashMap;

/// Extract a Cranelift type from a type expression (used in tuples).
fn type_from_expression(expr: &crate::ast::expression::Expression) -> CraneliftType {
    match &expr.node {
        ExpressionKind::Type(ty, _) => translate_type_kind(&ty.kind),
        _ => cl_types::I64, // Fallback to pointer size
    }
}

/// Compute byte offset and Cranelift type for a field within an aggregate.
pub fn field_layout(
    local_type: &TypeKind,
    field_idx: usize,
    type_definitions: &HashMap<String, TypeDefinition>,
) -> (i32, CraneliftType) {
    match local_type {
        TypeKind::Tuple(element_exprs) => {
            let mut offset: i32 = 0;
            for (i, elem_expr) in element_exprs.iter().enumerate() {
                let cl_ty = type_from_expression(elem_expr);
                if i == field_idx {
                    return (offset, cl_ty);
                }
                offset += cl_ty.bytes() as i32;
            }
            // Fallback if field_idx is out of range
            (offset, cl_types::I64)
        }
        TypeKind::Custom(name, _) => {
            if let Some(def) = type_definitions.get(name) {
                match def {
                    TypeDefinition::Struct(struct_def) => {
                        let mut offset: i32 = 0;
                        for (i, (_field_name, field_ty, _vis)) in
                            struct_def.fields.iter().enumerate()
                        {
                            let cl_ty = translate_type_kind(&field_ty.kind);
                            if i == field_idx {
                                return (offset, cl_ty);
                            }
                            offset += cl_ty.bytes() as i32;
                        }
                        (offset, cl_types::I64)
                    }
                    TypeDefinition::Enum(_) => {
                        // Discriminant is 8 bytes (I64) at offset 0; payload starts at offset 8
                        if field_idx == 0 {
                            (0, cl_types::I64) // discriminant
                        } else {
                            let payload_offset = 8 + ((field_idx - 1) as i32 * 8);
                            (payload_offset, cl_types::I64)
                        }
                    }
                    _ => {
                        // Fallback: assume 8-byte fields
                        ((field_idx as i32) * 8, cl_types::I64)
                    }
                }
            } else {
                // Type not found, assume 8-byte fields
                ((field_idx as i32) * 8, cl_types::I64)
            }
        }
        _ => {
            // Fallback: assume 8-byte fields (I64 pointer representation)
            ((field_idx as i32) * 8, cl_types::I64)
        }
    }
}

/// Compute total size of an aggregate for stack slot allocation.
pub fn aggregate_size(
    local_type: &TypeKind,
    type_definitions: &HashMap<String, TypeDefinition>,
) -> u32 {
    match local_type {
        TypeKind::Tuple(element_exprs) => {
            let mut total: u32 = 0;
            for elem_expr in element_exprs {
                let cl_ty = type_from_expression(elem_expr);
                total += cl_ty.bytes();
            }
            total
        }
        TypeKind::Custom(name, _) => {
            if let Some(def) = type_definitions.get(name) {
                match def {
                    TypeDefinition::Struct(struct_def) => {
                        let mut total: u32 = 0;
                        for (_field_name, field_ty, _vis) in &struct_def.fields {
                            let cl_ty = translate_type_kind(&field_ty.kind);
                            total += cl_ty.bytes();
                        }
                        total
                    }
                    TypeDefinition::Enum(enum_def) => {
                        // discriminant (8 bytes) + max payload size
                        let max_payload: u32 = enum_def
                            .variants
                            .values()
                            .map(|fields| {
                                fields
                                    .iter()
                                    .map(|ty| translate_type_kind(&ty.kind).bytes())
                                    .sum::<u32>()
                            })
                            .max()
                            .unwrap_or(0);
                        8 + max_payload
                    }
                    _ => 8, // Fallback
                }
            } else {
                8
            }
        }
        _ => 8, // Fallback: pointer size
    }
}
