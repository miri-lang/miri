// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Layout computation for aggregate types.
//!
//! Provides helpers to compute byte offsets and sizes for fields within
//! structs, tuples, and enums during Cranelift code generation.

use crate::ast::expression::ExpressionKind;
use crate::ast::types::TypeKind;
use crate::codegen::cranelift::types::translate_type_kind;
use crate::type_checker::context::{class_needs_vtable, collect_class_fields_all, TypeDefinition};
use cranelift_codegen::ir::Type as CraneliftType;
use std::collections::HashMap;

/// Align an offset up to the given alignment.
fn align_to(offset: i32, alignment: i32) -> i32 {
    if alignment <= 1 {
        return offset;
    }
    (offset + alignment - 1) & !(alignment - 1)
}

/// Get the alignment of a Cranelift type.
/// For scalars, alignment is equal to size.
fn type_alignment(cl_ty: CraneliftType) -> i32 {
    cl_ty.bytes() as i32
}

/// Extract a Cranelift type from a type expression (used in tuples).
fn type_from_expression(
    expr: &crate::ast::expression::Expression,
    ptr_ty: CraneliftType,
) -> CraneliftType {
    match &expr.node {
        ExpressionKind::Type(ty, _) => translate_type_kind(&ty.kind, ptr_ty),
        _ => ptr_ty, // Fallback to pointer size
    }
}

/// Compute byte offset and Cranelift type for a field within an aggregate.
pub fn field_layout(
    local_type: &TypeKind,
    field_idx: usize,
    type_definitions: &HashMap<String, TypeDefinition>,
    ptr_ty: CraneliftType,
) -> (i32, CraneliftType) {
    let ptr_size = ptr_ty.bytes() as i32;
    match local_type {
        TypeKind::Tuple(element_exprs) => {
            debug_assert!(
                field_idx < element_exprs.len(),
                "field_layout: tuple field index {} out of range (len {})",
                field_idx,
                element_exprs.len()
            );
            // Tuple layout: [elem_count: ptr_size][field0][field1]...
            // Fields start after the count header.
            let mut offset: i32 = ptr_size;
            for (i, elem_expr) in element_exprs.iter().enumerate() {
                let cl_ty = type_from_expression(elem_expr, ptr_ty);
                let alignment = type_alignment(cl_ty);
                offset = align_to(offset, alignment);
                if i == field_idx {
                    return (offset, cl_ty);
                }
                offset += cl_ty.bytes() as i32;
            }
            // Unreachable if debug_assert passed; fallback for release builds
            (offset, ptr_ty)
        }
        TypeKind::Custom(name, _) => {
            if let Some(def) = type_definitions.get(name) {
                match def {
                    TypeDefinition::Struct(struct_def) => {
                        debug_assert!(
                            field_idx < struct_def.fields.len(),
                            "field_layout: struct '{}' field index {} out of range (len {})",
                            name,
                            field_idx,
                            struct_def.fields.len()
                        );
                        let mut offset: i32 = 0;
                        for (i, (_field_name, field_ty, _vis)) in
                            struct_def.fields.iter().enumerate()
                        {
                            let cl_ty = translate_type_kind(&field_ty.kind, ptr_ty);
                            let alignment = type_alignment(cl_ty);
                            offset = align_to(offset, alignment);
                            if i == field_idx {
                                return (offset, cl_ty);
                            }
                            offset += cl_ty.bytes() as i32;
                        }
                        // Unreachable if debug_assert passed; fallback for release builds
                        (offset, ptr_ty)
                    }
                    TypeDefinition::Enum(_) => {
                        // Discriminant is pointer-sized at offset 0; payload starts at offset ptr_size
                        if field_idx == 0 {
                            (0, ptr_ty) // discriminant
                        } else {
                            // Enums currently use pointer-sized slots for all fields to simplify.
                            // Payload starts after the discriminant (pointer-sized).
                            let payload_offset = ptr_size + ((field_idx - 1) as i32 * ptr_size);
                            (payload_offset, ptr_ty)
                        }
                    }
                    TypeDefinition::Alias(alias_def) => {
                        // Resolve through the alias to the underlying type's layout
                        field_layout(
                            &alias_def.template.kind,
                            field_idx,
                            type_definitions,
                            ptr_ty,
                        )
                    }
                    TypeDefinition::Generic(_) => ((field_idx as i32) * ptr_size, ptr_ty),
                    TypeDefinition::Class(class_def) => {
                        // Class layout: [header: 16 bytes (malloc_ptr + RC)][vtable_ptr?][field0][field1]...
                        // For vtable-bearing classes, offset 0 is the vtable pointer (raw, not user-visible).
                        // User-declared fields start after the vtable pointer.
                        let all_fields = collect_class_fields_all(class_def, type_definitions);
                        let vtable_offset = if class_needs_vtable(name, type_definitions) {
                            ptr_size
                        } else {
                            0
                        };
                        let mut offset: i32 = vtable_offset;
                        for (i, (_field_name, field_info)) in all_fields.iter().enumerate() {
                            let cl_ty = translate_type_kind(&field_info.ty.kind, ptr_ty);
                            let alignment = type_alignment(cl_ty);
                            offset = align_to(offset, alignment);
                            if i == field_idx {
                                return (offset, cl_ty);
                            }
                            offset += cl_ty.bytes() as i32;
                        }
                        (offset, ptr_ty)
                    }
                    TypeDefinition::Trait(_) => ((field_idx as i32) * ptr_size, ptr_ty),
                }
            } else {
                // Type not found — assume pointer-sized fields
                ((field_idx as i32) * ptr_size, ptr_ty)
            }
        }
        // All other types: assume pointer-sized fields
        _ => ((field_idx as i32) * ptr_size, ptr_ty),
    }
}

/// Compute total size of an aggregate for stack slot allocation.
///
/// Returns the size in bytes needed to represent the given aggregate type
/// on the stack. For structs, this is the sum of field sizes. For enums,
/// it is the discriminant plus the largest variant payload.
pub fn aggregate_size(
    local_type: &TypeKind,
    type_definitions: &HashMap<String, TypeDefinition>,
    ptr_ty: CraneliftType,
) -> u32 {
    let ptr_size = ptr_ty.bytes();
    let mut max_align = ptr_size as i32;

    match local_type {
        TypeKind::Tuple(element_exprs) => {
            // Start after the count header (ptr_size bytes at offset 0)
            let mut total: i32 = ptr_size as i32;
            for elem_expr in element_exprs {
                let cl_ty = type_from_expression(elem_expr, ptr_ty);
                let alignment = type_alignment(cl_ty);
                max_align = max_align.max(alignment);
                total = align_to(total, alignment);
                total += cl_ty.bytes() as i32;
            }
            align_to(total, max_align) as u32
        }
        TypeKind::Custom(name, _) => {
            if let Some(def) = type_definitions.get(name) {
                match def {
                    TypeDefinition::Struct(struct_def) => {
                        let mut total: i32 = 0;
                        for (_field_name, field_ty, _vis) in &struct_def.fields {
                            let cl_ty = translate_type_kind(&field_ty.kind, ptr_ty);
                            let alignment = type_alignment(cl_ty);
                            max_align = max_align.max(alignment);
                            total = align_to(total, alignment);
                            total += cl_ty.bytes() as i32;
                        }
                        align_to(total, max_align) as u32
                    }
                    TypeDefinition::Enum(enum_def) => {
                        // discriminant + max payload size
                        // Each payload field is stored at pointer-size alignment to match
                        // field_layout which uses pointer-sized slots per field.
                        let max_payload: u32 = enum_def
                            .variants
                            .values()
                            .map(|fields| (fields.len() as u32) * ptr_size)
                            .max()
                            .unwrap_or(0);
                        ptr_size + max_payload
                    }
                    TypeDefinition::Alias(alias_def) => {
                        // Resolve through the alias to the underlying type's layout
                        aggregate_size(&alias_def.template.kind, type_definitions, ptr_ty)
                    }
                    // Generic, Class, Trait — pointer-sized
                    TypeDefinition::Generic(_)
                    | TypeDefinition::Class(_)
                    | TypeDefinition::Trait(_) => ptr_size,
                }
            } else {
                ptr_size
            }
        }
        // All non-aggregate types: pointer-sized
        _ => ptr_size,
    }
}
