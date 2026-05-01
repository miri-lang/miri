// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind, Terminator, TerminatorKind,
};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::{emit_to_string, lower_expression};

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

        // When a String part comes from a fresh allocation (e.g. a method call
        // like `s.to_upper()` that returns a new String temp), `emit_to_string`
        // below will create a Copy-wrapper temp and Perceus will IncRef the
        // source.  Without explicit cleanup the source temp would leak.
        // Capture it here so we can emit_temp_drop for it after the wrapper is
        // created — but only for fresh temps created during this f-string
        // lowering (index >= parts_watermark) and only for String-typed locals
        // (other types are not passed through a Copy-wrapper by emit_to_string).
        let string_source_local: Option<crate::mir::place::Local> =
            if matches!(part_kind, TypeKind::String) {
                match &part_op {
                    Operand::Copy(p) | Operand::Move(p) if p.local.0 >= parts_watermark => {
                        Some(p.local)
                    }
                    _ => None,
                }
            } else {
                None
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
        let mut call_args = vec![
            Operand::Copy(Place::new(old_acc)),
            Operand::Copy(Place::new(next_part)),
        ];
        if let Some(&al) = ctx.variable_map.get("allocator") {
            call_args.push(Operand::Copy(Place::new(al)));
        }
        let func_op = Operand::Constant(Box::new(Constant {
            span: expr.span,
            ty: Type::new(TypeKind::Identifier, expr.span),
            literal: Literal::Identifier("String_concat".to_string()),
        }));
        let result = ctx.push_temp(Type::new(TypeKind::String, expr.span), expr.span);
        let target_bb = ctx.new_basic_block();
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Call {
                func: func_op,
                args: call_args,
                out_args: Vec::new(),
                destination: Place::new(result),
                target: Some(target_bb),
            },
            expr.span,
        ));
        ctx.set_current_block(target_bb);

        // Release the consumed concat args in the successor block (AFTER the
        // Call returns).  Placing StorageDead here — not before set_terminator
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
