// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind};

use crate::mir::lowering::context::LoweringContext;

pub(crate) fn lower_type_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Type(ty, _is_nullable) = &expr.node else {
        unreachable!()
    };
    // Type expressions used as values (metatype operations)
    // Return an identifier representing the type name
    let type_name = format!("{}", ty);
    let constant = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Identifier, expr.span),
        literal: crate::ast::literal::Literal::Identifier(type_name),
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
