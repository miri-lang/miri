// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind};

use crate::mir::lowering::context::LoweringContext;

pub(crate) fn lower_identifier_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Identifier(name, _) = &expr.node else {
        unreachable!()
    };
    if let Some(&local) = ctx.variable_map.get(name.as_str()) {
        // If destination is provided, assign the variable to it
        if let Some(d) = dest {
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    d.clone(),
                    Rvalue::Use(Operand::Copy(Place::new(local))),
                ),
                span: expr.span,
            });
            Ok(Operand::Copy(d))
        } else {
            // Check if the type is Copy to determine Move vs Copy semantics
            let ty = &ctx.body.local_decls[local.0].ty;
            if ty.is_copy() {
                Ok(Operand::Copy(Place::new(local)))
            } else {
                Ok(Operand::Move(Place::new(local)))
            }
        }
    } else {
        // Assume global function/symbol
        // In a real compiler we would check if it exists in globals
        let constant = Operand::Constant(Box::new(Constant {
            span: expr.span,
            ty: Type::new(TypeKind::Symbol, expr.span),
            literal: crate::ast::literal::Literal::Symbol(name.clone()),
        }));

        if let Some(d) = dest {
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(constant.clone())),
                span: expr.span,
            });
            Ok(Operand::Copy(d))
        } else {
            Ok(constant)
        }
    }
}
