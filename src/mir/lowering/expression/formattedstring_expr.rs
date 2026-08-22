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
    // Formatted string: f"Hello, {name}! Age: {age}"
    //
    // Each part is converted to a String via `emit_to_string` and then
    // all parts are concatenated left-to-right via String_concat.
    use crate::ast::literal::Literal;

    if parts.is_empty() {
        // Empty f-string: produce an empty string literal.
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
        return Ok(Operand::Copy(Place::new(temp)));
    }

    // Watermark: any local created from here on is an intermediate temp that
    // belongs to this f-string expression.  We use it with emit_temp_drop to
    // release consumed parts after each concat without touching pre-existing
    // caller locals.
    let parts_watermark = ctx.body.local_decls.len();

    // Convert each part to a String Local.
    let mut string_parts: Vec<crate::mir::place::Local> = Vec::with_capacity(parts.len());

    for part in parts.iter() {
        let part_op = lower_expression(ctx, part, None)?;

        // Determine the type of this part.
        let part_kind = ctx
            .type_checker
            .get_type(part.id)
            .map(|t| t.kind.clone())
            .unwrap_or_else(|| match &part_op {
                Operand::Constant(c) => c.ty.kind.clone(),
                Operand::Copy(p) | Operand::Move(p) => {
                    ctx.body.local_decls[p.local.0].ty.kind.clone()
                }
            });

        // Fresh managed temps created during this f-string lowering must be dropped
        // after emit_to_string processes them. This includes:
        //
        // - String parts: emit_to_string creates a Copy-wrapper; Perceus IncRefs the
        //   source, so we must drop the source temp after the wrapper is created.
        // - Option/Result/enum parts: emit_to_string reads their fields via Copy and
        //   renders them to a fresh String. The original managed container (Option/Result/enum)
        //   temp must be explicitly dropped, not left to leak.
        //
        // We capture fresh temps (created during this f-string, index >= parts_watermark)
        // that are managed types. Scalars don't need dropping and don't produce such temps.
        let string_source_local: Option<crate::mir::place::Local> = match &part_kind {
            TypeKind::String
            | TypeKind::Option(_)
            | TypeKind::Result(_, _)
            | TypeKind::Custom(_, _) => match &part_op {
                Operand::Copy(p) | Operand::Move(p) if p.local.0 >= parts_watermark => {
                    Some(p.local)
                }
                _ => None,
            },
            _ => None,
        };

        let string_local = emit_to_string(ctx, part_op, &part_kind, &expr.span)?;

        // If there was a fresh String source temp, free it now that the
        // Copy-wrapper (string_local) holds the reference.  These StorageDead
        // statements are placed in the current block (before the first concat
        // terminator), which is safe because `string_local` — not the source —
        // is what gets passed to String_concat.
        if let Some(src) = string_source_local {
            if src != string_local {
                ctx.emit_temp_drop(src, parts_watermark, expr.span);
            }
        }

        string_parts.push(string_local);
    }

    // Concatenate all parts left-to-right via String_concat.
    let mut accumulator = string_parts[0];
    for &next_part in &string_parts[1..] {
        let old_acc = accumulator;
        let result = emit_string_concat(ctx, old_acc, next_part, &expr.span)?;

        // Release the consumed concat args in the successor block (AFTER the
        // Call returns).  Placing StorageDead here — not before the concat call
        // — ensures the strings remain alive while String_concat reads them.
        // Perceus will convert each StorageDead into a DecRef; when RC hits 0
        // emit_type_drop frees the allocation.
        ctx.emit_temp_drop(old_acc, parts_watermark, expr.span);
        ctx.emit_temp_drop(next_part, parts_watermark, expr.span);

        accumulator = result;
    }

    // DPS: if a destination was provided, write the final result into it
    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                d.clone(),
                Rvalue::Use(Operand::Copy(Place::new(accumulator))),
            ),
            span: expr.span,
        });
        // The accumulator temp is no longer needed once copied into the
        // destination.  Perceus IncRef'd accumulator for the Copy above, so
        // d and accumulator share the same RC (=2).  Dropping the temp here
        // brings it back to 1, leaving d as the sole owner.
        ctx.emit_temp_drop(accumulator, parts_watermark, expr.span);
        return Ok(Operand::Copy(d));
    }

    Ok(Operand::Copy(Place::new(accumulator)))
}
