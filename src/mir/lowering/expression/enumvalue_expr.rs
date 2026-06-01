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
    // EnumValue constructs an enum variant via `::` syntax, e.g. `Option::Some(v)`.
    let ExpressionKind::EnumValue(enum_expr, args) = &expr.node else {
        unreachable!()
    };
    let invalid = || {
        LoweringError::unsupported_expression(
            "Invalid EnumValue expression structure".to_string(),
            expr.span,
        )
    };
    let ExpressionKind::Member(type_expr, variant_expr) = &enum_expr.node else {
        return Err(invalid());
    };
    let ExpressionKind::Identifier(type_name, _) = &type_expr.node else {
        return Err(invalid());
    };
    let ExpressionKind::Identifier(variant_name, _) = &variant_expr.node else {
        return Err(invalid());
    };
    let Some(discriminant) = enum_variant_discriminant(ctx, type_name, variant_name) else {
        return Err(invalid());
    };

    emit_enum_aggregate(ctx, type_name, variant_name, discriminant, args, expr, dest)
}

/// Lower the variant args and emit the enum `Aggregate` into `dest` (or a temp).
fn emit_enum_aggregate(
    ctx: &mut LoweringContext,
    type_name: &str,
    variant_name: &str,
    discriminant: usize,
    args: &[Expression],
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let mut ops = vec![enum_discriminant_operand(discriminant, expr.span)];
    for arg in args {
        ops.push(lower_expression(ctx, arg, None)?);
    }

    // DPS: use the caller-provided destination, else allocate a fresh temp.
    let target = match dest {
        Some(d) => d,
        None => {
            let ty = resolve_type(ctx.type_checker, expr);
            Place::new(ctx.push_temp(ty, expr.span))
        }
    };
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            target.clone(),
            Rvalue::Aggregate(
                AggregateKind::Enum(Rc::from(type_name), Rc::from(variant_name)),
                ops,
            ),
        ),
        span: expr.span,
    });
    Ok(Operand::Copy(target))
}

/// Find the discriminant index of `variant_name` within enum `type_name`.
fn enum_variant_discriminant(
    ctx: &LoweringContext,
    type_name: &str,
    variant_name: &str,
) -> Option<usize> {
    let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) =
        ctx.type_checker.global_type_definitions.get(type_name)
    else {
        return None;
    };
    enum_def
        .variants
        .iter()
        .position(|(name, _)| name.as_str() == variant_name)
}

/// Build the i32 discriminant constant operand for an enum aggregate.
fn enum_discriminant_operand(discriminant: usize, span: crate::error::syntax::Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Int, span),
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I32(
            discriminant as i32,
        )),
    }))
}
