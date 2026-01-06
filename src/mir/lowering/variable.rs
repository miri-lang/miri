// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::ast::statement::VariableDeclaration;
use crate::error::syntax::Span;
use crate::mir::{Operand, Place, Rvalue, StatementKind as MirStatementKind, StorageClass};

use super::{lower_expression, resolve_type, LoweringContext};

pub fn lower_variable(ctx: &mut LoweringContext, decls: &[VariableDeclaration], span: &Span) {
    for decl in decls {
        let mut init_op = None;
        let var_ty;

        if let Some(init_expr) = &decl.initializer {
            let op = lower_expression(ctx, init_expr);

            // Try to get type from TypeChecker for the initializer expression
            if let Some(ty) = ctx.type_checker.get_type(init_expr.id) {
                var_ty = ty.clone();
            } else {
                // Fallback: infer from operand if constant or local
                var_ty = match &op {
                    Operand::Constant(c) => c.ty.clone(),
                    Operand::Copy(place) | Operand::Move(place) => {
                        ctx.body.local_decls[place.local.0].ty.clone()
                    }
                };
            }
            init_op = Some(op);
        } else if let Some(type_expr) = &decl.typ {
            var_ty = resolve_type(ctx.type_checker, type_expr);
        } else {
            panic!("Cannot determine type for variable '{}'", decl.name);
        }

        let local = ctx.push_local(decl.name.clone(), var_ty, span.clone());

        if decl.is_shared {
            ctx.body.local_decls[local.0].storage_class = StorageClass::GpuShared;
        }

        if let Some(op) = init_op {
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(local), Rvalue::Use(op)),
                span: span.clone(),
            });
        }
    }
}
