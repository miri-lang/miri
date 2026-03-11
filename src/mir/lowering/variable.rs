// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::statement::VariableDeclaration;
use crate::error::syntax::Span;
use crate::mir::{Place, Rvalue, StatementKind as MirStatementKind, StorageClass};

use super::{helpers::coerce_rvalue, lower_expression, resolve_type, LoweringContext};
use crate::error::lowering::LoweringError;

pub fn lower_variable(
    ctx: &mut LoweringContext,
    decls: &[VariableDeclaration],
    span: &Span,
) -> Result<(), LoweringError> {
    for decl in decls {
        let (var_ty, init_expr_opt, pre_lowered_op) = if let Some(type_expr) = &decl.typ {
            let ty = resolve_type(ctx.type_checker, type_expr);
            (ty, decl.initializer.as_ref(), None)
        } else if let Some(init_expr) = &decl.initializer {
            if let Some(ty) = ctx.type_checker.get_type(init_expr.id) {
                (ty.clone(), Some(init_expr), None)
            } else {
                // Must lower to infer type
                let op = lower_expression(ctx, init_expr, None)?;
                let ty = op.ty(&ctx.body).clone();
                (ty, Some(init_expr), Some(op))
            }
        } else {
            return Err(LoweringError::unsupported_expression(
                format!("Cannot determine type for variable '{}'", decl.name),
                *span,
            ));
        };

        // Clone var_ty only when needed for comparison; consume in final use
        let var_ty_kind = var_ty.kind.clone();
        let local = ctx.push_local(decl.name.clone(), var_ty, *span);

        if decl.is_shared {
            ctx.body.local_decls[local.0].storage_class = StorageClass::GpuShared;
        }

        if let Some(init_expr) = init_expr_opt {
            let dest = Place::new(local);

            // If we already lowered it (inference case), we must assign it now
            if let Some(op) = pre_lowered_op {
                // op.ty() == var_ty, so no cast needed
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(dest, Rvalue::Use(op)),
                    span: *span,
                });
            } else {
                // Check if we can use DPS (types match)
                let init_ty = ctx.type_checker.get_type(init_expr.id);
                // Comparison: ignore spans
                let types_match = init_ty.is_some_and(|ity| ity.kind == var_ty_kind);

                if types_match {
                    // Optimized path: write directly to variable
                    lower_expression(ctx, init_expr, Some(dest))?;
                } else {
                    // Fallback: create temp/use result, then cast/assign
                    let op = lower_expression(ctx, init_expr, None)?;
                    let op_ty = op.ty(&ctx.body).clone();

                    let rvalue = if op_ty.kind != var_ty_kind {
                        let target_ty = ctx.body.local_decls[local.0].ty.clone();
                        coerce_rvalue(op, &op_ty, &target_ty)
                    } else {
                        Rvalue::Use(op)
                    };

                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(dest, rvalue),
                        span: *span,
                    });
                }
            }
        }
    }
    Ok(())
}
