// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    AggregateKind, Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind,
};

use std::rc::Rc;

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::resolve_type;

pub(crate) fn lower_enumvalue_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::EnumValue(enum_expr, args) = &expr.node else {
        unreachable!()
    };
    // EnumValue is used for enum variant construction with :: syntax
    // e.g., Option::Some(value)
    // Extract the enum type name and variant from the expression
    if let ExpressionKind::Member(type_expr, variant_expr) = &enum_expr.node {
        if let ExpressionKind::Identifier(type_name, _) = &type_expr.node {
            if let ExpressionKind::Identifier(variant_name, _) = &variant_expr.node {
                if let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) =
                    ctx.type_checker.global_type_definitions.get(type_name)
                {
                    if let Some((discriminant, _)) = enum_def
                        .variants
                        .iter()
                        .enumerate()
                        .find(|(_, (name, _))| name.as_str() == variant_name)
                    {
                        // Create discriminant constant
                        let discr_op = Operand::Constant(Box::new(Constant {
                            span: expr.span,
                            ty: Type::new(TypeKind::Int, expr.span),
                            literal: crate::ast::literal::Literal::Integer(
                                crate::ast::literal::IntegerLiteral::I32(discriminant as i32),
                            ),
                        }));

                        // Lower all arguments
                        let mut ops = vec![discr_op];
                        for arg in args {
                            ops.push(lower_expression(ctx, arg, None)?);
                        }

                        // DPS: use the caller-provided destination if given,
                        // otherwise allocate a fresh temp.
                        let target = if let Some(d) = dest {
                            d
                        } else {
                            let ty = resolve_type(ctx.type_checker, expr);
                            Place::new(ctx.push_temp(ty, expr.span))
                        };

                        ctx.push_statement(crate::mir::Statement {
                            kind: MirStatementKind::Assign(
                                target.clone(),
                                Rvalue::Aggregate(
                                    AggregateKind::Enum(
                                        Rc::from(type_name.as_str()),
                                        Rc::from(variant_name.as_str()),
                                    ),
                                    ops,
                                ),
                            ),
                            span: expr.span,
                        });
                        return Ok(Operand::Copy(target));
                    }
                }
            }
        }
    }
    Err(LoweringError::unsupported_expression(
        "Invalid EnumValue expression structure".to_string(),
        expr.span,
    ))
}
