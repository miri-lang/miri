// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::error::lowering::LoweringError;
use crate::mir::{Operand, Place};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;

pub(crate) fn lower_namedargument_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::NamedArgument(_name, value_expr) = &expr.node else {
        unreachable!()
    };
    // Named argument: extract the value and lower it
    // The name is used by the type checker for struct field matching
    lower_expression(ctx, value_expr, dest)
}
