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

        let string_local = emit_to_string(ctx, part_op, &part_kind, &expr.span)?;
        string_parts.push(string_local);
    }

    // Concatenate all parts left-to-right via String_concat.
    let mut accumulator = string_parts[0];
    for &next_part in &string_parts[1..] {
        let mut call_args = vec![
            Operand::Copy(Place::new(accumulator)),
            Operand::Copy(Place::new(next_part)),
        ];
        if let Some(&al) = ctx.variable_map.get("allocator") {
            call_args.push(Operand::Copy(Place::new(al)));
        }
        let func_op = Operand::Constant(Box::new(Constant {
            span: expr.span,
            ty: Type::new(TypeKind::Symbol, expr.span),
            literal: Literal::Symbol("String_concat".to_string()),
        }));
        let result = ctx.push_temp(Type::new(TypeKind::String, expr.span), expr.span);
        let target_bb = ctx.new_basic_block();
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Call {
                func: func_op,
                args: call_args,
                destination: Place::new(result),
                target: Some(target_bb),
            },
            expr.span,
        ));
        ctx.set_current_block(target_bb);
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
        return Ok(Operand::Copy(d));
    }

    Ok(Operand::Copy(Place::new(accumulator)))
}
