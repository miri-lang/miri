// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::Expression;
use crate::error::lowering::LoweringError;
use crate::mir::{Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::helpers::resolve_type;

pub(crate) fn lower_super_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    // Super refers to the parent class instance.
    // It's represented as a special constant that the backend
    // will use to resolve parent class method calls.
    // The type checker ensures this is only used in a derived class.
    let ty = resolve_type(ctx.type_checker, expr);
    let constant = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty,
        literal: crate::ast::literal::Literal::Symbol("super".to_string()),
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
