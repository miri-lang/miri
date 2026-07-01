// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Layout computation for aggregate types.
//!
//! Provides helpers to compute byte offsets and sizes for fields within
//! structs, tuples, and enums during Cranelift code generation.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::TypeKind;
use crate::codegen::cranelift::types::translate_type_kind;
use crate::type_checker::context::{
    class_needs_vtable, collect_class_fields_all, ClassDefinition, EnumDefinition,
    GenericDefinition, StructDefinition, TypeDefinition,
};
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
///
/// Only `Type` nodes carry resolved type information in tuple elements.
/// Non-type expressions are pointer-sized fallbacks.
fn type_from_expression(
    expr: &crate::ast::expression::Expression,
    ptr_ty: CraneliftType,
) -> CraneliftType {
    match &expr.node {
        ExpressionKind::Type(ty, _) => translate_type_kind(&ty.kind, ptr_ty),
        ExpressionKind::Literal(_)
        | ExpressionKind::Identifier(..)
        | ExpressionKind::Binary(..)
        | ExpressionKind::Logical(..)
        | ExpressionKind::Unary(..)
        | ExpressionKind::Assignment(..)
        | ExpressionKind::Conditional(..)
        | ExpressionKind::Range(..)
        | ExpressionKind::Guard(..)
        | ExpressionKind::Member(..)
        | ExpressionKind::Index(..)
        | ExpressionKind::Call(..)
        | ExpressionKind::ImportPath(..)
        | ExpressionKind::GenericType(..)
        | ExpressionKind::TypeDeclaration(..)
        | ExpressionKind::EnumValue(..)
        | ExpressionKind::StructMember(..)
        | ExpressionKind::Lambda(..)
        | ExpressionKind::List(..)
        | ExpressionKind::Array(..)
        | ExpressionKind::Map(..)
        | ExpressionKind::Tuple(..)
        | ExpressionKind::Set(..)
        | ExpressionKind::Match(..)
        | ExpressionKind::FormattedString(..)
        | ExpressionKind::NamedArgument(..)
        | ExpressionKind::Super
        | ExpressionKind::Block(..)
        | ExpressionKind::Cast(..) => ptr_ty,
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
        TypeKind::Tuple(element_exprs) => tuple_field_layout(element_exprs, field_idx, ptr_ty),
        TypeKind::Custom(name, type_args) => {
            // Only substitute type arguments for compiler-known vector types (Vec2, Vec3, Vec4).
            // Other generic types (e.g. List<T>, List<Tuple<...>>) have field layout defined
            // by their type definition, not their type arguments.
            if let Some(dim) = crate::ast::types::vec_dim(name) {
                if let Some(args) = type_args {
                    if !args.is_empty() {
                        // Try to extract the element type from the first type argument
                        if let ExpressionKind::Type(elem_type, _) = &args[0].node {
                            debug_assert!(
                                field_idx < dim as usize,
                                "field_layout: vector '{}' field index {} out of bounds for dimension {}",
                                name,
                                field_idx,
                                dim
                            );
                            let elem_cl_ty = translate_type_kind(&elem_type.kind, ptr_ty);
                            let field_offset = (field_idx as i32) * elem_cl_ty.bytes() as i32;
                            return (field_offset, elem_cl_ty);
                        }
                    }
                }
            }
            custom_field_layout(
                name,
                type_args.as_deref(),
                field_idx,
                type_definitions,
                ptr_ty,
            )
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
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_) => ((field_idx as i32) * ptr_size, ptr_ty),
    }
}

fn tuple_field_layout(
    element_exprs: &[crate::ast::expression::Expression],
    field_idx: usize,
    ptr_ty: CraneliftType,
) -> (i32, CraneliftType) {
    let ptr_size = ptr_ty.bytes() as i32;
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

fn custom_field_layout(
    name: &str,
    type_args: Option<&[Expression]>,
    field_idx: usize,
    type_definitions: &HashMap<String, TypeDefinition>,
    ptr_ty: CraneliftType,
) -> (i32, CraneliftType) {
    let ptr_size = ptr_ty.bytes() as i32;
    let Some(def) = type_definitions.get(name) else {
        // Type not found — assume pointer-sized fields
        return ((field_idx as i32) * ptr_size, ptr_ty);
    };
    match def {
        TypeDefinition::Struct(struct_def) => {
            struct_field_layout(name, struct_def, field_idx, ptr_ty)
        }
        TypeDefinition::Enum(enum_def) => enum_field_layout(enum_def, field_idx, ptr_ty),
        TypeDefinition::Alias(alias_def) => field_layout(
            &alias_def.template.kind,
            field_idx,
            type_definitions,
            ptr_ty,
        ),
        TypeDefinition::Class(class_def) => class_field_layout(
            name,
            class_def,
            type_args,
            field_idx,
            type_definitions,
            ptr_ty,
        ),
        TypeDefinition::Generic(_) | TypeDefinition::Trait(_) => {
            ((field_idx as i32) * ptr_size, ptr_ty)
        }
    }
}

/// Resolve a class/struct field's declared type through the instantiation's
/// type arguments.
///
/// A field typed as a bare generic parameter (`TypeKind::Generic("T", …)` or
/// `TypeKind::Custom("T", None)`) is replaced by the concrete type argument at
/// the parameter's declaration position, so a monomorphized `Box<float>.value`
/// lays out at the concrete scalar width instead of a pointer slot. Fields with
/// a concrete type, or an unresolved generic (no matching argument), are
/// returned unchanged.
pub(crate) fn substitute_generic_field_kind(
    field_kind: &TypeKind,
    type_args: Option<&[Expression]>,
    def_generics: Option<&Vec<GenericDefinition>>,
) -> TypeKind {
    // Only a bare generic-parameter spelling can be substituted; a concrete
    // field type is returned unchanged.
    let param_name = if let TypeKind::Generic(name, _, _) = field_kind {
        name.as_str()
    } else if let TypeKind::Custom(name, None) = field_kind {
        name.as_str()
    } else {
        return field_kind.clone();
    };
    let (Some(generics), Some(args)) = (def_generics, type_args) else {
        return field_kind.clone();
    };
    let Some(pos) = generics.iter().position(|g| g.name == param_name) else {
        return field_kind.clone();
    };
    if let Some(ExpressionKind::Type(ty, _)) = args.get(pos).map(|a| &a.node) {
        ty.kind.clone()
    } else {
        field_kind.clone()
    }
}

fn struct_field_layout(
    name: &str,
    struct_def: &StructDefinition,
    field_idx: usize,
    ptr_ty: CraneliftType,
) -> (i32, CraneliftType) {
    debug_assert!(
        field_idx < struct_def.fields.len(),
        "field_layout: struct '{}' field index {} out of range (len {})",
        name,
        field_idx,
        struct_def.fields.len()
    );
    let mut offset: i32 = 0;
    for (i, (_field_name, field_ty, _vis)) in struct_def.fields.iter().enumerate() {
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

fn enum_field_layout(
    _enum_def: &EnumDefinition,
    field_idx: usize,
    ptr_ty: CraneliftType,
) -> (i32, CraneliftType) {
    let ptr_size = ptr_ty.bytes() as i32;
    // Discriminant is pointer-sized at offset 0; payload starts at offset ptr_size.
    // Enums currently use pointer-sized slots for all fields to simplify layout.
    if field_idx == 0 {
        (0, ptr_ty)
    } else {
        let payload_offset = ptr_size + ((field_idx - 1) as i32 * ptr_size);
        (payload_offset, ptr_ty)
    }
}

fn class_field_layout(
    name: &str,
    class_def: &ClassDefinition,
    type_args: Option<&[Expression]>,
    field_idx: usize,
    type_definitions: &HashMap<String, TypeDefinition>,
    ptr_ty: CraneliftType,
) -> (i32, CraneliftType) {
    let ptr_size = ptr_ty.bytes() as i32;
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
        // A generic-parameter field is monomorphized to its concrete type
        // argument so it lays out at the instantiation's scalar width.
        let field_kind = substitute_generic_field_kind(
            &field_info.ty.kind,
            type_args,
            class_def.generics.as_ref(),
        );
        let cl_ty = translate_type_kind(&field_kind, ptr_ty);
        let alignment = type_alignment(cl_ty);
        offset = align_to(offset, alignment);
        if i == field_idx {
            return (offset, cl_ty);
        }
        offset += cl_ty.bytes() as i32;
    }
    (offset, ptr_ty)
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
    match local_type {
        TypeKind::Tuple(element_exprs) => tuple_aggregate_size(element_exprs, ptr_ty),
        TypeKind::Custom(name, _) => custom_aggregate_size(name, type_definitions, ptr_ty),
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
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_) => ptr_size,
    }
}

/// Size of a tuple aggregate: `[count_header][field0][field1]...`, padded to
/// the maximum field alignment.
fn tuple_aggregate_size(
    element_exprs: &[crate::ast::expression::Expression],
    ptr_ty: CraneliftType,
) -> u32 {
    let ptr_size = ptr_ty.bytes() as i32;
    let mut max_align = ptr_size;
    let mut total = ptr_size;
    for elem_expr in element_exprs {
        let cl_ty = type_from_expression(elem_expr, ptr_ty);
        let alignment = type_alignment(cl_ty);
        max_align = max_align.max(alignment);
        total = align_to(total, alignment);
        total += cl_ty.bytes() as i32;
    }
    align_to(total, max_align) as u32
}

/// Size of a `Custom(name, _)` aggregate by dispatching on the resolved
/// `TypeDefinition`. Unknown names and definitions that carry no on-stack
/// payload (classes, traits, generics) fall back to a pointer slot.
fn custom_aggregate_size(
    name: &str,
    type_definitions: &HashMap<String, TypeDefinition>,
    ptr_ty: CraneliftType,
) -> u32 {
    let ptr_size = ptr_ty.bytes();
    match type_definitions.get(name) {
        Some(TypeDefinition::Struct(struct_def)) => struct_aggregate_size(struct_def, ptr_ty),
        Some(TypeDefinition::Enum(enum_def)) => enum_aggregate_size(enum_def, ptr_size),
        Some(TypeDefinition::Alias(alias_def)) => {
            aggregate_size(&alias_def.template.kind, type_definitions, ptr_ty)
        }
        None
        | Some(TypeDefinition::Generic(_))
        | Some(TypeDefinition::Class(_))
        | Some(TypeDefinition::Trait(_)) => ptr_size,
    }
}

/// Size of a struct aggregate: sum of field sizes with per-field alignment,
/// padded to the maximum encountered alignment (at least ptr-sized).
fn struct_aggregate_size(struct_def: &StructDefinition, ptr_ty: CraneliftType) -> u32 {
    let ptr_size = ptr_ty.bytes() as i32;
    let mut max_align = ptr_size;
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

/// Size of an enum aggregate: a ptr-sized discriminant followed by the
/// largest variant payload. Each payload field uses a pointer-sized slot to
/// match the convention in [`field_layout`].
fn enum_aggregate_size(enum_def: &EnumDefinition, ptr_size: u32) -> u32 {
    let max_payload = enum_def
        .variants
        .values()
        .map(|fields| (fields.len() as u32) * ptr_size)
        .max()
        .unwrap_or(0);
    ptr_size + max_payload
}
