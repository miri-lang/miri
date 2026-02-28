// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::error::lowering::LoweringError;
use crate::mir::{Operand, Place};

use crate::mir::lowering::context::LoweringContext;

pub(crate) fn lower_structmember_expr(
    _ctx: &mut LoweringContext,
    expr: &Expression,
    _dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::StructMember(_, _) = &expr.node else {
        unreachable!()
    };
    // StructMember is primarily used in struct declarations, not runtime
    // If encountered at runtime, it's likely an error in the AST structure
    Err(LoweringError::unsupported_expression(
        "StructMember expressions are only valid in struct declarations".to_string(),
        expr.span,
    ))
}
