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
    StructDefinition, TypeDefinition,
};
use crate::type_checker::generics::substitute_generic_field_kind;
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
        TypeDefinition::Enum(enum_def) => enum_field_layout(enum_def, type_args, field_idx, ptr_ty),
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

/// Where field `field_idx` of an enum value sits: the discriminant is field 0,
/// and payload field `k` of whichever variant the value holds is field `k + 1`.
///
/// A read of a payload field does not know which variant it reads — the match
/// arm that reached it does, but the projection only carries the index — so
/// the offset must not depend on the variant. Every field therefore occupies
/// one slot of [`enum_payload_slot_size`], the same for all variants of the
/// enum at this instantiation. The type returned is a pointer-sized word; a
/// reader loads at the width of the binding it fills.
fn enum_field_layout(
    enum_def: &EnumDefinition,
    type_args: Option<&[Expression]>,
    field_idx: usize,
    ptr_ty: CraneliftType,
) -> (i32, CraneliftType) {
    let slot = enum_payload_slot_size(enum_def, type_args, ptr_ty) as i32;
    ((field_idx as i32) * slot, ptr_ty)
}

/// The width of every slot in an enum value: the discriminant's and each
/// payload field's.
///
/// It is the widest payload any variant carries at this instantiation, and
/// never narrower than a pointer, so a 128-bit payload gets a slot it fits in
/// and every other enum keeps pointer-sized slots. A payload spelled as a type
/// parameter takes the width of the argument bound to it; one with no bound
/// argument is pointer-sized.
///
/// This is the one authority for enum payload offsets: construction, the
/// field reads and writes of a match, and the drop path all lay a value out
/// by it, so none of them can disagree about where a field lives.
pub fn enum_payload_slot_size(
    enum_def: &EnumDefinition,
    type_args: Option<&[Expression]>,
    ptr_ty: CraneliftType,
) -> u32 {
    enum_def
        .variants
        .values()
        .flatten()
        .map(|field_ty| {
            let stored = enum_payload_field_kind(enum_def, &field_ty.kind, type_args);
            translate_type_kind(&stored, ptr_ty).bytes()
        })
        .fold(ptr_ty.bytes(), u32::max)
}

/// The kind an enum payload field declared as `declared` stores at the
/// instantiation `type_args`: a payload spelled as a type parameter takes the
/// argument bound to it, and any other payload is stored as declared. A
/// parameter with no bound argument is returned unresolved.
///
/// The slot width above, and the drop path's choice of which fields to
/// release, both resolve payloads through this, so the offsets a value is
/// written at and the offsets it is released from cannot disagree.
pub fn enum_payload_field_kind(
    enum_def: &EnumDefinition,
    declared: &TypeKind,
    type_args: Option<&[Expression]>,
) -> TypeKind {
    substitute_generic_field_kind(declared, type_args, enum_def.generics.as_ref())
}

/// Where each field of a class instance sits, and how much memory the fields
/// need.
///
/// Both answers come from one walk so they cannot disagree. Sizing an instance
/// from anything other than the fields it holds — the widths of the values a
/// constructor happens to hand over, say — gives an object whose allocation is
/// smaller than the offsets written into it, and the write past the end lands
/// in whatever the allocator put next.
pub struct ClassPayloadLayout {
    /// Offset and Cranelift type of each field, in the order
    /// [`collect_class_fields_all`] lists them.
    pub fields: Vec<(i32, CraneliftType)>,
    /// Bytes the payload occupies, counting the vtable slot a dispatching
    /// class carries ahead of its first field.
    pub size: u32,
}

/// Lay out one class instance's payload. See [`ClassPayloadLayout`].
pub fn class_payload_layout(
    name: &str,
    class_def: &ClassDefinition,
    type_args: Option<&[Expression]>,
    type_definitions: &HashMap<String, TypeDefinition>,
    ptr_ty: CraneliftType,
) -> ClassPayloadLayout {
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
    let mut fields = Vec::with_capacity(all_fields.len());
    let mut offset: i32 = vtable_offset;
    let mut max_align = ptr_size;
    for (_field_name, field_info) in all_fields.iter() {
        // A generic-parameter field is monomorphized to its concrete type
        // argument so it lays out at the instantiation's scalar width.
        let field_kind = substitute_generic_field_kind(
            &field_info.ty.kind,
            type_args,
            class_def.generics.as_ref(),
        );
        let cl_ty = translate_type_kind(&field_kind, ptr_ty);
        let alignment = type_alignment(cl_ty);
        max_align = max_align.max(alignment);
        offset = align_to(offset, alignment);
        fields.push((offset, cl_ty));
        offset += cl_ty.bytes() as i32;
    }
    ClassPayloadLayout {
        fields,
        size: align_to(offset, max_align) as u32,
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
    let layout = class_payload_layout(name, class_def, type_args, type_definitions, ptr_ty);
    match layout.fields.get(field_idx) {
        Some(&placed) => placed,
        // An index past the last field belongs to no declared field. The slot
        // after the payload is the closest thing to an answer, and a caller
        // reaching it is already asking about a field the class does not have.
        None => (layout.size as i32, ptr_ty),
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
    match local_type {
        TypeKind::Tuple(element_exprs) => tuple_aggregate_size(element_exprs, ptr_ty),
        TypeKind::Custom(name, type_args) => {
            custom_aggregate_size(name, type_args.as_deref(), type_definitions, ptr_ty)
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
    type_args: Option<&[Expression]>,
    type_definitions: &HashMap<String, TypeDefinition>,
    ptr_ty: CraneliftType,
) -> u32 {
    let ptr_size = ptr_ty.bytes();
    match type_definitions.get(name) {
        Some(TypeDefinition::Struct(struct_def)) => struct_aggregate_size(struct_def, ptr_ty),
        Some(TypeDefinition::Enum(enum_def)) => enum_aggregate_size(enum_def, type_args, ptr_ty),
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

/// Size of an enum aggregate: the discriminant slot followed by as many
/// payload slots as the variant with the most fields carries, each slot as wide
/// as [`enum_payload_slot_size`] says.
fn enum_aggregate_size(
    enum_def: &EnumDefinition,
    type_args: Option<&[Expression]>,
    ptr_ty: CraneliftType,
) -> u32 {
    let max_fields = enum_def.variants.values().map(Vec::len).max().unwrap_or(0);
    enum_payload_slot_size(enum_def, type_args, ptr_ty) * (1 + max_fields as u32)
}
