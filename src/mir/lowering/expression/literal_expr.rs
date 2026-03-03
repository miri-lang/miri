// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind};

use crate::mir::lowering::context::LoweringContext;

pub(crate) fn lower_literal_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Literal(lit) = &expr.node else {
        unreachable!()
    };
    // Prefer type checker's resolved type for proper context-aware typing
    // Only infer from literal if type checker doesn't have a type
    let ty = if let Some(resolved) = ctx.type_checker.get_type(expr.id) {
        resolved.clone()
    } else {
        match lit {
            crate::ast::literal::Literal::Integer(int_lit) => {
                // Preserve specific integer type from the literal
                use crate::ast::literal::IntegerLiteral;
                match int_lit {
                    IntegerLiteral::I8(_) => Type::new(TypeKind::I8, expr.span),
                    IntegerLiteral::I16(_) => Type::new(TypeKind::I16, expr.span),
                    IntegerLiteral::I32(_) => Type::new(TypeKind::I32, expr.span),
                    IntegerLiteral::I64(_) => Type::new(TypeKind::I64, expr.span),
                    IntegerLiteral::I128(_) => Type::new(TypeKind::I128, expr.span),
                    IntegerLiteral::U8(_) => Type::new(TypeKind::U8, expr.span),
                    IntegerLiteral::U16(_) => Type::new(TypeKind::U16, expr.span),
                    IntegerLiteral::U32(_) => Type::new(TypeKind::U32, expr.span),
                    IntegerLiteral::U64(_) => Type::new(TypeKind::U64, expr.span),
                    IntegerLiteral::U128(_) => Type::new(TypeKind::U128, expr.span),
                }
            }
            crate::ast::literal::Literal::Boolean(_) => Type::new(TypeKind::Boolean, expr.span),
            crate::ast::literal::Literal::String(_) => Type::new(TypeKind::String, expr.span),
            crate::ast::literal::Literal::Float(float_lit) => {
                // Preserve specific float type from the literal
                use crate::ast::literal::FloatLiteral;
                match float_lit {
                    FloatLiteral::F32(_) => Type::new(TypeKind::F32, expr.span),
                    FloatLiteral::F64(_) => Type::new(TypeKind::F64, expr.span),
                }
            }
            crate::ast::literal::Literal::Identifier(_) => Type::new(TypeKind::Identifier, expr.span),
            crate::ast::literal::Literal::Regex(_) => {
                // Regex literals are represented as strings internally
                Type::new(TypeKind::String, expr.span)
            }
            crate::ast::literal::Literal::None => {
                // None represents the absence of a value (null/nil)
                // Use Void type since it's the unit type in Miri
                Type::new(TypeKind::Void, expr.span)
            }
        }
    };

    let constant = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty,
        literal: lit.clone(),
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
