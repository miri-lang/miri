// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    AggregateKind, Constant, Dimension, GpuIntrinsic, MathIntrinsic, Operand, Place, Rvalue,
    StatementKind as MirStatementKind,
};

use std::rc::Rc;

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::control_flow::lower_call;
use crate::mir::lowering::expression::{lower_expression, testing_intrinsic};
use crate::mir::lowering::helpers::resolve_type;

pub(crate) fn lower_call_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Call(func, args) = &expr.node else {
        unreachable!()
    };

    if let Some(op) = try_lower_option_some(ctx, func, args, expr, dest.clone())? {
        return Ok(op);
    }

    if let Some(op) = try_lower_testing_intrinsic(ctx, func, args, expr, dest.clone())? {
        return Ok(op);
    }

    if let Some(op) = try_lower_gpu_or_math_intrinsic(ctx, func, args, expr, dest.clone())? {
        return Ok(op);
    }

    if let Some(op) = try_lower_enum_variant(ctx, func, args, expr, dest.clone())? {
        return Ok(op);
    }

    lower_call(ctx, &expr.span, expr.id, func, args, dest)
}

fn try_lower_option_some(
    ctx: &mut LoweringContext,
    func: &Expression,
    args: &[Expression],
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let is_option_some = match &func.node {
        ExpressionKind::Identifier(name, _) => name == "Some",
        ExpressionKind::Member(enum_expr, variant_expr) => {
            if let ExpressionKind::Identifier(type_name, _) = &enum_expr.node {
                if let ExpressionKind::Identifier(variant_name, _) = &variant_expr.node {
                    type_name == crate::ast::types::OPTION_TYPE_NAME && variant_name == "Some"
                } else {
                    false
                }
            } else {
                false
            }
        }
        _ => false,
    };

    if !is_option_some || args.len() != 1 {
        return Ok(None);
    }

    let arg_watermark = ctx.body.local_decls.len();
    let inner_val = lower_expression(ctx, &args[0], None)?;
    let target = if let Some(d) = dest {
        d
    } else {
        let ty = resolve_type(ctx.type_checker, expr);
        Place::new(ctx.push_temp(ty, expr.span))
    };
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            target.clone(),
            Rvalue::Aggregate(AggregateKind::Option, vec![inner_val.clone()]),
        ),
        span: expr.span,
    });
    if let Operand::Copy(ref p) | Operand::Move(ref p) = inner_val {
        ctx.emit_temp_drop(p.local, arg_watermark, expr.span);
    }
    Ok(Some(Operand::Copy(target)))
}

fn try_lower_testing_intrinsic(
    ctx: &mut LoweringContext,
    func: &Expression,
    args: &[Expression],
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    if let ExpressionKind::Identifier(name, _) = &func.node {
        if testing_intrinsic::is_testing_intrinsic(name.as_str())
            && testing_intrinsic::is_from_testing_module(ctx, name.as_str())
        {
            return Ok(Some(testing_intrinsic::lower_testing_intrinsic(
                ctx, expr, name.as_str(), args, dest,
            )?));
        }
    }
    Ok(None)
}

fn try_lower_gpu_or_math_intrinsic(
    ctx: &mut LoweringContext,
    func: &Expression,
    args: &[Expression],
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    if let ExpressionKind::Identifier(name, _) = &func.node {
        if let Some(op) = try_lower_gpu_intrinsic(ctx, name, expr, dest.clone())? {
            return Ok(Some(op));
        }

        if let Some(op) = try_lower_math_intrinsic(ctx, name, args, expr, dest)? {
            return Ok(Some(op));
        }
    }
    Ok(None)
}

fn try_lower_gpu_intrinsic(
    ctx: &mut LoweringContext,
    name: &str,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let intrinsic_rvalue = match name {
        "gpu_thread_idx_x" => Some(Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(Dimension::X))),
        "gpu_thread_idx_y" => Some(Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(Dimension::Y))),
        "gpu_thread_idx_z" => Some(Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(Dimension::Z))),
        "gpu_block_idx_x" => Some(Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(Dimension::X))),
        "gpu_block_idx_y" => Some(Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(Dimension::Y))),
        "gpu_block_idx_z" => Some(Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(Dimension::Z))),
        _ => None,
    };

    let Some(rvalue) = intrinsic_rvalue else {
        return Ok(None);
    };

    let (target, ret_op) = if let Some(ref d) = dest {
        (d.clone(), Operand::Copy(d.clone()))
    } else {
        let temp = ctx.push_temp(Type::new(TypeKind::Int, expr.span), expr.span);
        (Place::new(temp), Operand::Copy(Place::new(temp)))
    };
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(target, rvalue),
        span: expr.span,
    });
    Ok(Some(ret_op))
}

fn try_lower_math_intrinsic(
    ctx: &mut LoweringContext,
    name: &str,
    args: &[Expression],
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let Some(intrinsic) = MathIntrinsic::from_name(name) else {
        return Ok(None);
    };

    let is_from_math_module = ctx
        .type_checker
        .get_variable_module(name)
        .map(|m| m == "system.math")
        .unwrap_or(false);

    if !is_from_math_module {
        return Ok(None);
    }

    let mut arg_ops = Vec::with_capacity(args.len());
    for arg in args {
        arg_ops.push(lower_expression(ctx, arg, None)?);
    }

    let return_ty = resolve_type(ctx.type_checker, expr);
    let (target, ret_op) = if let Some(ref d) = dest {
        (d.clone(), Operand::Copy(d.clone()))
    } else {
        let temp = ctx.push_temp(return_ty, expr.span);
        (Place::new(temp), Operand::Copy(Place::new(temp)))
    };

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(target, Rvalue::MathIntrinsic(intrinsic, arg_ops)),
        span: expr.span,
    });
    Ok(Some(ret_op))
}

fn try_lower_enum_variant(
    ctx: &mut LoweringContext,
    func: &Expression,
    args: &[Expression],
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let ExpressionKind::Member(enum_expr, variant_expr) = &func.node else {
        return Ok(None);
    };
    let ExpressionKind::Identifier(type_name, _) = &enum_expr.node else {
        return Ok(None);
    };
    let ExpressionKind::Identifier(variant_name, _) = &variant_expr.node else {
        return Ok(None);
    };
    let Some(discriminant) = enum_call_discriminant(ctx, type_name, variant_name) else {
        return Ok(None);
    };

    let op = emit_enum_variant_call(ctx, type_name, variant_name, discriminant, args, expr, dest)?;
    Ok(Some(op))
}

/// Find the discriminant index of `variant_name` within enum `type_name`.
fn enum_call_discriminant(
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

/// Lower the variant args and emit the enum `Aggregate`, dropping each fresh
/// arg temp (the args' RC is donated into the aggregate).
fn emit_enum_variant_call(
    ctx: &mut LoweringContext,
    type_name: &str,
    variant_name: &str,
    discriminant: usize,
    args: &[Expression],
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let discr_op = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Int, expr.span),
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I32(
            discriminant as i32,
        )),
    }));

    let arg_watermark = ctx.body.local_decls.len();
    let mut ops = vec![discr_op];
    for arg in args {
        ops.push(lower_expression(ctx, arg, None)?);
    }

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
                ops.clone(),
            ),
        ),
        span: expr.span,
    });
    for op in ops.iter().skip(1) {
        if let Operand::Copy(ref p) | Operand::Move(ref p) = op {
            ctx.emit_temp_drop(p.local, arg_watermark, expr.span);
        }
    }
    Ok(Operand::Copy(target))
}
