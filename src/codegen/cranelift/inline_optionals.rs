// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Optionals held as their bytes: a one-byte presence tag, zero when absent,
//! and the payload beside it, laid out as the value-class authority says.
//!
//! MIR keeps optionals semantic — `Some(x)` built, `None` assigned, the
//! optional switched on or compared with `None`, its payload opened as field
//! 0 — and this is where each of those meets the bytes:
//!
//! - `Some(x)` sets the tag and stores `x` as the payload;
//! - the absent value clears every byte, the tag among them, so two absent
//!   optionals hold the same bytes;
//! - a switch and a comparison with `None` read the tag;
//! - a cast between optionals of two scalars keeps the tag and converts the
//!   payload; one from the absent value's placeholder type (`void?`, a word,
//!   null when absent) reads that word layer by layer.
//!
//! An optional held as its bytes has no count, so no release reaches it.

use crate::ast::literal::Literal;
use crate::ast::types::TypeKind;
use crate::codegen::cranelift::translator::{FunctionTranslator, ModuleCtx, TypeCtx};
use crate::codegen::cranelift::value_locals::{clear_value, copy_value, scalar_type, ValueBuild};
use crate::error::CodegenError;
use crate::mir::option_layout::is_absent_placeholder;
use crate::mir::type_facts::{FieldRepr, ScalarKind, ValueField, ValueLayout};
use crate::mir::{Local, Operand};
use cranelift_codegen::ir::{types as cl_types, InstBuilder, MemFlags, Value};
use cranelift_frontend::{FunctionBuilder, Variable};
use std::collections::HashMap;

impl FunctionTranslator<'_> {
    /// The presence tag of the optional `operand` reads, loaded as a byte —
    /// zero when absent — when that optional is held as its bytes; `None`
    /// for any other operand.
    ///
    /// An optional held as a word is its own discriminant, null when absent;
    /// one held as its bytes is tested by its tag.
    pub(crate) fn inline_optional_tag(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        operand: &Operand,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Option<Value>, CodegenError> {
        if !matches!(operand, Operand::Copy(_) | Operand::Move(_)) {
            return Ok(None);
        }
        let kind = Self::operand_projected_kind(operand, type_ctx)?;
        let Some(layout) = type_ctx.value_layout(&kind) else {
            return Ok(None);
        };
        let Some(tag) = layout.fields.first().filter(|_| layout.is_optional()) else {
            return Ok(None);
        };
        let offset = tag.offset as i32;
        let address = Self::value_operand_address(builder, ctx, operand, locals, type_ctx, None)?;
        Ok(Some(builder.ins().load(
            cl_types::I8,
            MemFlags::new(),
            address,
            offset,
        )))
    }

    /// Fill the bytes `target` describes with `operand` cast to their type.
    ///
    /// A value laid out as the destination is copied unchanged. An optional
    /// of a scalar cast to an optional of another scalar keeps its tag, and
    /// its payload is converted the way a plain assignment converts it.
    pub(super) fn cast_whole_value(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        target: ValueBuild,
        operand: &Operand,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if is_absent(operand) {
            return Self::store_absent_value(builder, target);
        }
        let kind = Self::operand_projected_kind(operand, type_ctx)?;
        if is_absent_placeholder(&kind) {
            let word = Self::translate_operand(builder, ctx, operand, locals, type_ctx, None)?;
            return Self::store_placeholder_optional(builder, word, target);
        }
        let source_layout = type_ctx.value_layout(&kind);
        let source = Self::value_operand_address(builder, ctx, operand, locals, type_ctx, None)?;
        if source_layout.as_ref() == Some(target.layout) {
            copy_value(builder, source, target.address, target.layout);
            return Ok(());
        }
        let source_layout = source_layout.ok_or_else(|| Self::uncastable(&kind, target.layout))?;
        Self::convert_optional_payload(builder, source, &source_layout, target)
            .ok_or_else(|| Self::uncastable(&kind, target.layout))?
    }

    /// Copy the optional of a scalar laid out as `source_layout` at `source`
    /// into the optional of another scalar `target` describes: its tag as it
    /// is, its payload converted the way a plain assignment converts it.
    /// `None` when either is no optional of a scalar.
    pub(super) fn convert_optional_payload(
        builder: &mut FunctionBuilder,
        source: Value,
        source_layout: &ValueLayout,
        target: ValueBuild,
    ) -> Option<Result<(), CodegenError>> {
        let (Some(from_tag), Some(from), Some(to_tag), Some(to)) = (
            source_layout
                .fields
                .first()
                .filter(|_| source_layout.is_optional()),
            source_layout.projected_field(0),
            target
                .layout
                .fields
                .first()
                .filter(|_| target.layout.is_optional()),
            target.layout.projected_field(0),
        ) else {
            return None;
        };
        let (FieldRepr::Scalar(from_scalar), FieldRepr::Scalar(to_scalar)) = (&from.repr, &to.repr)
        else {
            return None;
        };
        let presence = builder.ins().load(
            cl_types::I8,
            MemFlags::new(),
            source,
            from_tag.offset as i32,
        );
        builder.ins().store(
            MemFlags::new(),
            presence,
            target.address,
            to_tag.offset as i32,
        );
        let payload = builder.ins().load(
            scalar_type(*from_scalar),
            MemFlags::new(),
            source,
            from.offset as i32,
        );
        let from_unsigned = matches!(from_scalar, ScalarKind::Unsigned(_));
        Some(Self::store_scalar(
            builder,
            payload,
            (*to_scalar, from_unsigned),
            target.address,
            to.offset,
        ))
    }

    /// Fill the optional held as its bytes at `target` from `word`, a value
    /// of the type an absent value is spelled at before it meets a slot
    /// (`void?`, `void??`, ...): null when absent, otherwise a box whose one
    /// word holds the next layer the same way.
    pub(super) fn store_placeholder_optional(
        builder: &mut FunctionBuilder,
        word: Value,
        target: ValueBuild,
    ) -> Result<(), CodegenError> {
        Self::store_absent_value(builder, target)?;
        let (Some(tag), Some(payload)) = (
            target.layout.fields.first(),
            target.layout.projected_field(0),
        ) else {
            return Ok(());
        };
        let FieldRepr::Value(inner) = &payload.repr else {
            // A payload no placeholder can hold: only absence is spelled.
            return Ok(());
        };
        let present = builder.create_block();
        let done = builder.create_block();
        builder.ins().brif(word, present, &[], done, &[]);
        builder.switch_to_block(present);
        builder.seal_block(present);
        Self::store_tag(builder, target.address, tag, true)?;
        let pointer_type = builder.func.dfg.value_type(word);
        let next = builder.ins().load(pointer_type, MemFlags::new(), word, 0);
        let inner_address = builder
            .ins()
            .iadd_imm(target.address, i64::from(payload.offset));
        let inner_target = ValueBuild {
            address: inner_address,
            layout: inner,
        };
        if inner.is_optional() {
            Self::store_placeholder_optional(builder, next, inner_target)?;
        }
        builder.ins().jump(done, &[]);
        builder.switch_to_block(done);
        builder.seal_block(done);
        Ok(())
    }

    /// The refusal of a cast of a `kind` into bytes laid out as `layout`.
    pub(super) fn uncastable(kind: &TypeKind, layout: &ValueLayout) -> CodegenError {
        CodegenError::Internal(format!(
            "`{kind}` is cast into a value of {} bytes laid out apart from it",
            layout.size
        ))
    }

    /// Whether `operand` reads a value of the type an absent value is spelled
    /// at before it meets a slot.
    pub(super) fn reads_absent_placeholder(
        operand: &Operand,
        type_ctx: &TypeCtx,
    ) -> Result<bool, CodegenError> {
        if !matches!(operand, Operand::Copy(_) | Operand::Move(_)) {
            return Ok(false);
        }
        Ok(is_absent_placeholder(&Self::operand_projected_kind(
            operand, type_ctx,
        )?))
    }

    /// Build a present optional held as its bytes: its tag set, and its one
    /// operand stored as the payload beside it.
    pub(super) fn build_present_optional(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        build: ValueBuild,
        operands: &[Operand],
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let (Some(tag), Some(payload), [operand]) = (
            build.layout.fields.first(),
            build.layout.projected_field(0),
            operands,
        ) else {
            return Err(CodegenError::Internal(format!(
                "a `Some` of {} values is built into bytes laid out as no optional",
                operands.len()
            )));
        };
        Self::store_tag(builder, build.address, tag, true)?;
        Self::store_member(
            builder,
            ctx,
            build.address,
            payload,
            operand,
            locals,
            type_ctx,
        )
    }

    /// Write the absent value into the bytes `target` describes: every byte
    /// cleared, so two absent values hold the same bytes. An optional's tag
    /// is among them, and reads absent. A value no optional spells is the
    /// placeholder a field takes before its initializer runs: cleared bytes.
    pub(super) fn store_absent_value(
        builder: &mut FunctionBuilder,
        target: ValueBuild,
    ) -> Result<(), CodegenError> {
        clear_value(builder, target.address, target.layout);
        Ok(())
    }

    /// Store `is_present` into `tag`, an optional's presence tag.
    pub(super) fn store_tag(
        builder: &mut FunctionBuilder,
        address: Value,
        tag: &ValueField,
        is_present: bool,
    ) -> Result<(), CodegenError> {
        if tag.repr != FieldRepr::OptionTag {
            return Err(CodegenError::Internal(
                "an optional's tag is written into bytes laid out as no optional".to_string(),
            ));
        }
        let value = builder.ins().iconst(cl_types::I8, i64::from(is_present));
        builder
            .ins()
            .store(MemFlags::new(), value, address, tag.offset as i32);
        Ok(())
    }
}

/// Whether `operand` is the absent value written as a constant.
pub(super) fn is_absent(operand: &Operand) -> bool {
    matches!(operand, Operand::Constant(constant) if matches!(constant.literal, Literal::None))
}
