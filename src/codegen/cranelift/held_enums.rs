// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Enums held as their bytes: a tag word, then one column per payload
//! position that every variant storing a field there shares (see
//! [`LayoutShape::Enum`]).
//!
//! A value is built in place: its tag, each field of its variant into the
//! member of the column that variant stores, a column overlaying members of
//! different shapes cleared first when that member does not fill it, and
//! every column past them cleared. A column of mixed members is copied whole,
//! so no copy of the value carries bytes an earlier value left there; a value
//! nested in a column keeps its padding unspecified, as any held value does,
//! and a copy copies its members only. Two
//! values are equal when their tags are and the fields of the variant the tag
//! names are; a column read on its own does not say which variant wrote it.

use crate::ast::types::Type;
use crate::codegen::cranelift::translator::{FunctionTranslator, ModuleCtx, TypeCtx};
use crate::codegen::cranelift::types::translate_type_kind;
use crate::codegen::cranelift::value_locals::{
    load_at, repr_scalar_members, scalar_type, store_at, zero_scalar, ValueBuild,
};
use crate::error::CodegenError;
use crate::mir::type_facts::{FieldRepr, LayoutShape, ScalarKind, TypeDefinition, ValueLayout};
use crate::mir::{Local, Operand, PlaceElem};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{types as cl_types, InstBuilder, Value};
use cranelift_frontend::{FunctionBuilder, Variable};
use std::collections::HashMap;

impl<'a> FunctionTranslator<'a> {
    /// Build the value of `variant` of the enum `enum_name` from `operands` —
    /// its discriminant, then its fields — into the bytes `build` describes.
    pub(crate) fn build_enum_in_place(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        build: ValueBuild,
        (enum_name, variant): (&str, &str),
        operands: &[Operand],
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let discriminant = variant_discriminant(enum_name, variant, type_ctx)?;
        let members = variant_members(build.layout, discriminant)?;
        let (Some(tag), Some((tag_operand, payloads))) =
            (build.layout.fields.first(), operands.split_first())
        else {
            return Err(enum_error("an enum value is built with no discriminant"));
        };
        if payloads.len() != members.len() {
            return Err(enum_error(&format!(
                "`{enum_name}.{variant}` is built with {} fields for {}",
                payloads.len(),
                members.len()
            )));
        }
        let address = build.address;
        let tag_member = (tag.offset, &tag.repr);
        Self::store_member(
            builder,
            ctx,
            address,
            tag_member,
            tag_operand,
            locals,
            type_ctx,
        )?;
        for (position, (operand, &member)) in payloads.iter().zip(members).enumerate() {
            let column = column_member(build.layout, position, member)?;
            clear_column_around(builder, address, build.layout, position, column.1);
            Self::store_member(builder, ctx, address, column, operand, locals, type_ctx)?;
        }
        for column in build.layout.fields.iter().skip(1 + payloads.len()) {
            clear_member(builder, address, column.offset, &column.repr);
        }
        Ok(())
    }

    /// Whether the two enums held as their bytes at `lhs` and `rhs`, laid out
    /// as `layout` from `offset` on, are equal: the same variant, with every
    /// field of it equal as `==` compares that field.
    pub(crate) fn held_enum_equality(
        builder: &mut FunctionBuilder,
        (lhs, rhs): (Value, Value),
        layout: &ValueLayout,
        offset: u32,
    ) -> Result<Value, CodegenError> {
        let LayoutShape::Enum { variants } = &layout.shape else {
            return Err(enum_error(
                "an enum is compared through a layout of no enum",
            ));
        };
        let left = load_tag(builder, lhs, layout, offset)?;
        let right = load_tag(builder, rhs, layout, offset)?;
        let same_variant = builder.ins().icmp(IntCC::Equal, left, right);
        let mut fields_equal = builder.ins().iconst(cl_types::I8, 1);
        for (discriminant, members) in variants.iter().enumerate() {
            let mut equal = builder.ins().iconst(cl_types::I8, 1);
            for (position, &member) in members.iter().enumerate() {
                let (at, repr) = column_member(layout, position, member)?;
                let field = (offset + at, repr);
                let field_equal = Self::held_member_equality(builder, (lhs, rhs), field)?;
                equal = builder.ins().band(equal, field_equal);
            }
            let is_variant = builder
                .ins()
                .icmp_imm_s(IntCC::Equal, left, discriminant as i64);
            fields_equal = builder.ins().select(is_variant, equal, fields_equal);
        }
        Ok(builder.ins().band(same_variant, fields_equal))
    }
}

/// The tag of the enum laid out as `layout` at `offset` from `address`.
pub(crate) fn load_tag(
    builder: &mut FunctionBuilder,
    address: Value,
    layout: &ValueLayout,
    offset: u32,
) -> Result<Value, CodegenError> {
    let Some((at, FieldRepr::Scalar(tag))) =
        (layout.fields.first()).map(|field| (field.offset, &field.repr))
    else {
        return Err(enum_error("an enum held as its bytes has no tag word"));
    };
    let at =
        i32::try_from(offset + at).map_err(|_| enum_error("an enum's tag offset overflows"))?;
    Ok(load_at(builder, scalar_type(*tag), address, at))
}

/// Which member of each column the variant numbered `discriminant` stores
/// its fields as, read from `layout`.
pub(crate) fn variant_members(
    layout: &ValueLayout,
    discriminant: usize,
) -> Result<&[usize], CodegenError> {
    let LayoutShape::Enum { variants } = &layout.shape else {
        return Err(enum_error("an enum is built into a layout of no enum"));
    };
    variants
        .get(discriminant)
        .map(Vec::as_slice)
        .ok_or_else(|| enum_error(&format!("an enum has no variant {discriminant}")))
}

/// The offset of payload position `position` in `layout`, and member
/// `member` of its column.
pub(crate) fn column_member(
    layout: &ValueLayout,
    position: usize,
    member: usize,
) -> Result<(u32, &FieldRepr), CodegenError> {
    let Some(column) = layout.fields.get(position + 1) else {
        return Err(enum_error(&format!(
            "an enum has no payload column {position}"
        )));
    };
    let FieldRepr::Overlay(members) = &column.repr else {
        return Err(enum_error(&format!(
            "enum payload position {position} is no column"
        )));
    };
    members
        .get(member)
        .map(|repr| (column.offset, repr))
        .ok_or_else(|| enum_error(&format!("column {position} has no member {member}")))
}

/// The member of an enum column of `members` a read into a value of
/// `expected` reaches: the nested value of its layout, or the scalar it is
/// loaded as. Refused when no type says, or when no member is one.
pub(crate) fn member_read_as<'l>(
    members: &'l [FieldRepr],
    expected: Option<&Type>,
    type_ctx: &TypeCtx,
) -> Result<&'l FieldRepr, CodegenError> {
    let expected = expected.ok_or_else(|| {
        CodegenError::Internal("an enum payload column is read with no type to read it as".into())
    })?;
    let held = type_ctx.value_layout(&expected.kind);
    let loaded = translate_type_kind(&expected.kind, type_ctx.ptr_type);
    let is_read_as = |member: &&FieldRepr| match member {
        FieldRepr::Value(layout) => held.as_ref() == Some(layout),
        FieldRepr::Scalar(scalar) => held.is_none() && scalar_type(*scalar) == loaded,
        FieldRepr::OptionTag | FieldRepr::Overlay(_) => false,
    };
    members.iter().find(is_read_as).ok_or_else(|| {
        CodegenError::Internal(format!(
            "an enum payload column holds no `{}` to read",
            expected.kind
        ))
    })
}

/// The one value an enum payload column of `members` holds that `elem`
/// projects into: a projection reaches inside a value, and a column with one
/// value among its members can hold no other there.
pub(crate) fn projected_column_value<'l>(
    members: &'l [FieldRepr],
    elem: &PlaceElem,
) -> Result<&'l ValueLayout, CodegenError> {
    let mut values = members.iter().filter_map(|member| match member {
        FieldRepr::Value(layout) => Some(layout),
        FieldRepr::Scalar(_) | FieldRepr::OptionTag | FieldRepr::Overlay(_) => None,
    });
    match (values.next(), values.next()) {
        (Some(layout), None) => Ok(layout),
        (None, _) | (Some(_), Some(_)) => Err(enum_error(&format!(
            "`{elem:?}` is projected out of an enum payload column holding no single value"
        ))),
    }
}

/// Append the scalar members of an enum column of `size` bytes holding
/// `column`, placed at `offset`, to `members`: the grid its members share, or
/// the unsigned integers tiling it.
pub(crate) fn collect_column_members(
    column: &[FieldRepr],
    size: u32,
    offset: u32,
    members: &mut Vec<(u32, ScalarKind)>,
) {
    let mut grids = column.iter().map(repr_scalar_members);
    let shared = grids
        .next()
        .filter(|first| grids.all(|grid| grid == *first));
    let placed = shared.unwrap_or_else(|| integer_tiles(size));
    members.extend(placed.into_iter().map(|(at, scalar)| (offset + at, scalar)));
}

/// The unsigned integers tiling `size` bytes from offset 0: words, then the
/// halves, quarters and byte the tail needs.
fn integer_tiles(size: u32) -> Vec<(u32, ScalarKind)> {
    let mut tiles = Vec::new();
    let mut at = 0;
    for width in [8, 4, 2, 1] {
        while size - at >= width {
            tiles.push((at, ScalarKind::Unsigned(width)));
            at += width;
        }
    }
    tiles
}

/// The discriminant of `variant` of the enum `enum_name`: its position among
/// the variants in the order the definition keeps them, sorted by name — not
/// the order they are declared in — the numbering every enum is built and
/// matched by.
fn variant_discriminant(
    enum_name: &str,
    variant: &str,
    type_ctx: &TypeCtx,
) -> Result<usize, CodegenError> {
    let Some(TypeDefinition::Enum(definition)) = type_ctx.facts.definitions().get(enum_name) else {
        return Err(enum_error(&format!("`{enum_name}` names no enum")));
    };
    definition
        .variants
        .keys()
        .position(|name| name == variant)
        .ok_or_else(|| enum_error(&format!("`{enum_name}` has no variant `{variant}`")))
}

/// Clear payload column `position` of `layout` at `address` before `member`
/// is written into it, when the column overlays members of different shapes
/// and `member` does not fill it: such a column is copied whole, and the
/// bytes past `member` an earlier value left would travel with every copy.
fn clear_column_around(
    builder: &mut FunctionBuilder,
    address: Value,
    layout: &ValueLayout,
    position: usize,
    member: &FieldRepr,
) {
    let Some(column) = layout.fields.get(position + 1) else {
        return;
    };
    let FieldRepr::Overlay(members) = &column.repr else {
        return;
    };
    let (column_size, _) = column.repr.size_and_align();
    let member_bytes: u32 = repr_scalar_members(member)
        .iter()
        .map(|(_, scalar)| scalar.size())
        .sum();
    if members.len() > 1 && member_bytes < column_size {
        clear_member(builder, address, column.offset, &column.repr);
    }
}

/// Clear the member `repr` at `offset` from `address`, every scalar it
/// flattens to stored as its zero.
fn clear_member(builder: &mut FunctionBuilder, address: Value, offset: u32, repr: &FieldRepr) {
    for (at, scalar) in repr_scalar_members(repr) {
        let zero = zero_scalar(builder, scalar);
        store_at(builder, zero, address, (offset + at) as i32);
    }
}

fn enum_error(message: &str) -> CodegenError {
    CodegenError::Internal(message.to_string())
}

#[cfg(test)]
mod column_copy_tests {
    use super::collect_column_members;
    use crate::mir::type_facts::{FieldRepr, ScalarKind};

    fn copied(column: &[FieldRepr], size: u32) -> Vec<(u32, ScalarKind)> {
        let mut members = Vec::new();
        collect_column_members(column, size, 16, &mut members);
        members
    }

    #[test]
    fn a_column_whose_members_agree_is_copied_as_their_scalars() {
        let float = FieldRepr::Scalar(ScalarKind::Float(8));
        assert_eq!(copied(&[float], 8), vec![(16, ScalarKind::Float(8))]);
    }

    #[test]
    fn a_column_of_mixed_members_is_copied_as_the_integers_tiling_it() {
        let column = [
            FieldRepr::Scalar(ScalarKind::Signed(8)),
            FieldRepr::Scalar(ScalarKind::Float(8)),
            FieldRepr::Scalar(ScalarKind::Signed(1)),
        ];
        assert_eq!(
            copied(&column, 15),
            vec![
                (16, ScalarKind::Unsigned(8)),
                (24, ScalarKind::Unsigned(4)),
                (28, ScalarKind::Unsigned(2)),
                (30, ScalarKind::Unsigned(1)),
            ]
        );
    }
}
