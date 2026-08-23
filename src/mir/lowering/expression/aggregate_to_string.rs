// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Renders a class or struct value as `Name(field=value, ...)` for assertion
//! failure messages.
//!
//! This rendering is reachable only from the assertion lowering. String
//! interpolation deliberately does not accept a class or struct: naming every
//! field is a debugging view, and letting `f"{p}"` produce it would settle the
//! question of what a type's displayed form is before that question has been
//! answered.

use crate::ast::literal::Literal;
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::{emit_string_concat, emit_to_string};
use crate::mir::place::Local;
use crate::mir::{Constant, Operand, Place, PlaceElem, Rvalue, StatementKind as MirStatementKind};

/// A field's name and declared type, in declaration order.
type FieldList = Vec<(String, Type)>;

/// Render `operand` as `Name(field=value, ...)`, returning the String local.
///
/// Returns `None` when the named type is neither a class nor a struct, leaving
/// the caller to pick another rendering.
pub(super) fn emit_aggregate_debug_string(
    ctx: &mut LoweringContext,
    operand: Operand,
    type_name: &str,
    span: &Span,
) -> Option<Result<Local, LoweringError>> {
    let fields = aggregate_fields(ctx, type_name)?;
    Some(render_aggregate(ctx, operand, type_name, &fields, span))
}

/// The declared fields of a class or struct, or `None` for any other type.
fn aggregate_fields(ctx: &LoweringContext, type_name: &str) -> Option<FieldList> {
    match ctx.type_checker.type_definitions().get(type_name) {
        Some(crate::type_checker::context::TypeDefinition::Class(class_def)) => Some(
            class_def
                .fields
                .iter()
                .map(|(name, info)| (name.clone(), info.ty.clone()))
                .collect(),
        ),
        Some(crate::type_checker::context::TypeDefinition::Struct(struct_def)) => Some(
            struct_def
                .fields
                .iter()
                .map(|(name, ty, _)| (name.clone(), ty.clone()))
                .collect(),
        ),
        _ => None,
    }
}

fn render_aggregate(
    ctx: &mut LoweringContext,
    operand: Operand,
    type_name: &str,
    fields: &FieldList,
    span: &Span,
) -> Result<Local, LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let base = crate::mir::lowering::helpers::ensure_place(ctx, operand, *span);

    let mut acc = emit_string_literal(ctx, &format!("{}(", type_name), *span);

    for (field_idx, (field_name, field_ty)) in fields.iter().enumerate() {
        if field_idx > 0 {
            let separator = emit_string_literal(ctx, ", ", *span);
            acc = concat_and_release(ctx, acc, separator, watermark, span)?;
        }

        let label = emit_string_literal(ctx, &format!("{}=", field_name), *span);
        acc = concat_and_release(ctx, acc, label, watermark, span)?;

        let rendered = render_field(ctx, &base, field_idx, field_ty, watermark, span)?;
        acc = concat_and_release(ctx, acc, rendered, watermark, span)?;
    }

    let close = emit_string_literal(ctx, ")", *span);
    concat_and_release(ctx, acc, close, watermark, span)
}

/// Render one field, reading it at its own declared type.
///
/// `Operand::ty` ignores projections, so a field read straight from a
/// projected place is read at the base slot's width; materializing through a
/// typed temp first is what keeps a float from rendering as its bit pattern.
fn render_field(
    ctx: &mut LoweringContext,
    base: &Place,
    field_idx: usize,
    field_ty: &Type,
    watermark: usize,
    span: &Span,
) -> Result<Local, LoweringError> {
    let field_temp = ctx.push_temp(field_ty.clone(), *span);
    let mut field_place = base.clone();
    field_place.projection.push(PlaceElem::Field(field_idx));
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(field_temp),
            Rvalue::Use(Operand::Copy(field_place)),
        ),
        span: *span,
    });

    let rendered = emit_to_string(
        ctx,
        Operand::Copy(Place::new(field_temp)),
        &field_ty.kind,
        span,
    )?;

    if ctx.is_perceus_managed(&field_ty.kind) {
        ctx.emit_temp_drop(field_temp, watermark, *span);
    }
    Ok(rendered)
}

/// Concatenate two rendered strings and release both inputs.
fn concat_and_release(
    ctx: &mut LoweringContext,
    lhs: Local,
    rhs: Local,
    watermark: usize,
    span: &Span,
) -> Result<Local, LoweringError> {
    let joined = emit_string_concat(ctx, lhs, rhs, span)?;
    ctx.emit_temp_drop(lhs, watermark, *span);
    ctx.emit_temp_drop(rhs, watermark, *span);
    Ok(joined)
}

/// Materialise a String constant into a fresh temp.
fn emit_string_literal(ctx: &mut LoweringContext, text: &str, span: Span) -> Local {
    let ty = Type::new(TypeKind::String, span);
    let temp = ctx.push_temp(ty.clone(), span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(temp),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty,
                literal: Literal::String(text.to_string()),
            }))),
        ),
        span,
    });
    temp
}
