// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::{emit_string_concat, emit_to_string, lower_expression};

pub(crate) fn lower_formattedstring_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::FormattedString(parts) = &expr.node else {
        unreachable!()
    };

    if parts.is_empty() {
        return lower_empty_formattedstring(ctx, expr);
    }

    let parts_watermark = ctx.body.local_decls.len();

    let mut string_parts = Vec::with_capacity(parts.len());
    for part in parts {
        let string_local = lower_formattedstring_part(ctx, part, expr, parts_watermark)?;
        string_parts.push(string_local);
    }

    let accumulator = concat_formattedstring_parts(ctx, &string_parts, expr, parts_watermark)?;

    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                d.clone(),
                Rvalue::Use(Operand::Copy(Place::new(accumulator))),
            ),
            span: expr.span,
        });
        ctx.emit_temp_drop(accumulator, parts_watermark, expr.span);
        return Ok(Operand::Copy(d));
    }

    Ok(Operand::Copy(Place::new(accumulator)))
}

fn lower_empty_formattedstring(
    ctx: &mut LoweringContext,
    expr: &Expression,
) -> Result<Operand, LoweringError> {
    use crate::ast::literal::Literal;

    let ty = Type::new(TypeKind::String, expr.span);
    let temp = ctx.push_temp(ty.clone(), expr.span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(temp),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span: expr.span,
                ty,
                literal: Literal::String(String::new()),
            }))),
        ),
        span: expr.span,
    });
    Ok(Operand::Copy(Place::new(temp)))
}

fn lower_formattedstring_part(
    ctx: &mut LoweringContext,
    part: &Expression,
    parent_expr: &Expression,
    parts_watermark: usize,
) -> Result<crate::mir::place::Local, LoweringError> {
    let part_op = lower_expression(ctx, part, None)?;

    let part_kind = ctx
        .type_checker
        .get_type(part.id)
        .map(|t| t.kind.clone())
        .unwrap_or_else(|| match &part_op {
            Operand::Constant(c) => c.ty.kind.clone(),
            Operand::Copy(p) | Operand::Move(p) => ctx.body.local_decls[p.local.0].ty.kind.clone(),
        });

    let string_source_local: Option<crate::mir::place::Local> = match &part_kind {
        TypeKind::String
        | TypeKind::Option(_)
        | TypeKind::Result(_, _)
        | TypeKind::Custom(_, _) => match &part_op {
            Operand::Copy(p) | Operand::Move(p) if p.local.0 >= parts_watermark => Some(p.local),
            _ => None,
        },
        _ => None,
    };

    let string_local = emit_to_string(ctx, part_op, &part_kind, &parent_expr.span)?;

    if let Some(src) = string_source_local {
        if src != string_local {
            ctx.emit_temp_drop(src, parts_watermark, parent_expr.span);
        }
    }

    Ok(string_local)
}

fn concat_formattedstring_parts(
    ctx: &mut LoweringContext,
    string_parts: &[crate::mir::place::Local],
    parent_expr: &Expression,
    parts_watermark: usize,
) -> Result<crate::mir::place::Local, LoweringError> {
    let mut accumulator = string_parts[0];
    for &next_part in &string_parts[1..] {
        let old_acc = accumulator;
        let result = emit_string_concat(ctx, old_acc, next_part, &parent_expr.span)?;

        ctx.emit_temp_drop(old_acc, parts_watermark, parent_expr.span);
        ctx.emit_temp_drop(next_part, parts_watermark, parent_expr.span);

        accumulator = result;
    }
    Ok(accumulator)
}
