// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{vec_dim, Type, TypeKind};
use crate::ast::BuiltinCollectionKind;
use crate::error::lowering::LoweringError;
use crate::mir::backend::gpu::GpuAtomicOp;
use crate::mir::backend::BackendMetadata;
use crate::mir::{
    AggregateKind, Constant, Dimension, GpuIntrinsic, MathIntrinsic, Operand, Place, Rvalue,
    StatementKind as MirStatementKind,
};

use std::rc::Rc;

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::control_flow::lower_call;
use crate::mir::lowering::expression::{lower_expression, testing_intrinsic};
use crate::mir::lowering::helpers::{gpu_math_return_type, resolve_type};

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

    if let Some(op) = try_lower_gpu_barrier(ctx, func, expr)? {
        return Ok(op);
    }

    if let Some(op) = try_lower_gpu_shuffle_down(ctx, func, args, expr, dest.clone())? {
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
                ctx,
                expr,
                name.as_str(),
                args,
                dest,
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

        if let Some(op) = try_lower_atomic_builtin(ctx, name, args, expr, dest.clone())? {
            return Ok(Some(op));
        }

        if let Some(op) = try_lower_vector_builtin(ctx, name, args, expr, dest.clone())? {
            return Ok(Some(op));
        }

        if let Some(op) = try_lower_math_intrinsic(ctx, name, args, expr, dest)? {
            return Ok(Some(op));
        }
    }
    Ok(None)
}

/// Lowers `kernel.barrier()` to a `SyncThreads` intrinsic statement. The call
/// returns `void`, so it is emitted purely for its workgroup-synchronization
/// side effect; the assignment target is a throwaway void temp the WGSL backend
/// renders as a bare `workgroupBarrier();`.
fn try_lower_gpu_barrier(
    ctx: &mut LoweringContext,
    func: &Expression,
    expr: &Expression,
) -> Result<Option<Operand>, LoweringError> {
    let ExpressionKind::Member(obj, prop) = &func.node else {
        return Ok(None);
    };
    let ExpressionKind::Identifier(obj_name, _) = &obj.node else {
        return Ok(None);
    };
    if obj_name != crate::ast::types::KERNEL_CONTEXT_IDENT
        && obj_name != crate::ast::types::GPU_CONTEXT_DEPRECATED_IDENT
    {
        return Ok(None);
    }
    let ExpressionKind::Identifier(prop_name, _) = &prop.node else {
        return Ok(None);
    };
    if prop_name != "barrier" {
        return Ok(None);
    }

    let void_temp = ctx.push_temp(Type::new(TypeKind::Void, expr.span), expr.span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(void_temp),
            Rvalue::GpuIntrinsic(GpuIntrinsic::SyncThreads),
        ),
        span: expr.span,
    });
    Ok(Some(Operand::Copy(Place::new(void_temp))))
}

/// Lowers `kernel.warp.shuffle_down(value, offset)` to a `ShuffleDown` intrinsic.
/// The offset MUST be a compile-time integer literal (validated here).
fn try_lower_gpu_shuffle_down(
    ctx: &mut LoweringContext,
    func: &Expression,
    args: &[Expression],
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let ExpressionKind::Member(obj, prop) = &func.node else {
        return Ok(None);
    };
    let ExpressionKind::Member(kernel_expr, warp_expr) = &obj.node else {
        return Ok(None);
    };
    let ExpressionKind::Identifier(kernel_name, _) = &kernel_expr.node else {
        return Ok(None);
    };
    if kernel_name != crate::ast::types::KERNEL_CONTEXT_IDENT
        && kernel_name != crate::ast::types::GPU_CONTEXT_DEPRECATED_IDENT
    {
        return Ok(None);
    }
    let ExpressionKind::Identifier(warp_name, _) = &warp_expr.node else {
        return Ok(None);
    };
    if warp_name != "warp" {
        return Ok(None);
    }
    let ExpressionKind::Identifier(prop_name, _) = &prop.node else {
        return Ok(None);
    };
    if prop_name != "shuffle_down" {
        return Ok(None);
    }

    if args.len() != 2 {
        return Err(LoweringError::unsupported_expression(
            "kernel.warp.shuffle_down requires 2 arguments (value, offset)".to_string(),
            expr.span,
        ));
    }

    // Lower the value operand
    let value_op = lower_expression(ctx, &args[0], None)?;

    // Extract the compile-time literal offset
    let offset = match &args[1].node {
        ExpressionKind::Literal(lit) => {
            use crate::ast::literal::Literal;
            match lit {
                Literal::Integer(i) => {
                    let val = i.to_i128() as u32;
                    if val > 128 {
                        return Err(LoweringError::custom(
                            format!(
                                "shuffle offset {} exceeds the maximum subgroup size (128)",
                                val
                            ),
                            args[1].span,
                            None,
                        ));
                    }
                    val
                }
                _ => {
                    return Err(LoweringError::unsupported_expression(
                        "shuffle offset must be a compile-time literal".to_string(),
                        args[1].span,
                    ));
                }
            }
        }
        _ => {
            return Err(LoweringError::unsupported_expression(
                "shuffle offset must be a compile-time literal".to_string(),
                args[1].span,
            ));
        }
    };

    let result_ty = resolve_type(ctx.type_checker, &args[0]);
    let (target, ret_op) = if let Some(ref d) = dest {
        (d.clone(), Operand::Copy(d.clone()))
    } else {
        let temp = ctx.push_temp(result_ty, expr.span);
        (Place::new(temp), Operand::Copy(Place::new(temp)))
    };

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            target,
            Rvalue::GpuIntrinsic(GpuIntrinsic::ShuffleDown(Box::new(value_op), offset)),
        ),
        span: expr.span,
    });

    Ok(Some(ret_op))
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

/// True if `elem_ty` is `Atomic<u32>` or `Atomic<i32>` (the only WGSL-portable
/// atomic element types).
fn is_u32_or_i32_atomic_element(elem_ty: &TypeKind) -> bool {
    let TypeKind::Custom(name, Some(inner_args)) = elem_ty else {
        return false;
    };
    if name != crate::ast::types::ATOMIC_TYPE_NAME || inner_args.len() != 1 {
        return false;
    }
    match &inner_args[0].node {
        ExpressionKind::Type(inner_ty, _) => {
            matches!(&inner_ty.kind, TypeKind::U32 | TypeKind::I32)
        }
        _ => false,
    }
}

/// True if `buffer_ty` is an `Array<Atomic<u32|i32>, N>` (in either the
/// post-normalization `Custom("Array", ...)` or the pre-normalization
/// `Array(elem, size)` form).
fn is_atomic_u32_i32_buffer(buffer_ty: &TypeKind) -> bool {
    let elem_node = match buffer_ty {
        TypeKind::Custom(coll_name, Some(coll_args))
            if BuiltinCollectionKind::from_name(coll_name)
                == Some(BuiltinCollectionKind::Array)
                && !coll_args.is_empty() =>
        {
            &coll_args[0].node
        }
        TypeKind::Array(elem_expr, _) => &elem_expr.node,
        _ => return false,
    };
    matches!(elem_node, ExpressionKind::Type(elem_ty, _) if is_u32_or_i32_atomic_element(&elem_ty.kind))
}

/// Records that the current kernel body needs 32-bit integer atomics.
fn mark_atomic_capability(ctx: &mut LoweringContext) {
    use crate::mir::backend::gpu::GpuCapability;
    if let Some(BackendMetadata::Gpu(gpu_meta)) = &mut ctx.body.backend_metadata {
        if !gpu_meta
            .required_capabilities
            .contains(&GpuCapability::AtomicInt32)
        {
            gpu_meta
                .required_capabilities
                .push(GpuCapability::AtomicInt32);
        }
    }
}

/// Lowers a compiler-recognized atomic builtin (`atomic_add`, …,
/// `atomic_compare_exchange`) into an `Rvalue::AtomicOp`. Dispatched by name
/// and by the first-argument type, which must be `Array<Atomic<u32|i32>, N>`.
fn try_lower_atomic_builtin(
    ctx: &mut LoweringContext,
    name: &str,
    args: &[Expression],
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let Some(op) = GpuAtomicOp::from_builtin_name(name) else {
        return Ok(None);
    };

    // `compare_exchange` takes (buffer, index, expected, new); the rest take
    // (buffer, index, value).
    let min_args = if op == GpuAtomicOp::CompareExchange {
        4
    } else {
        3
    };
    if args.len() < min_args {
        return Ok(None);
    }

    let buffer_ty = resolve_type(ctx.type_checker, &args[0]);
    if !is_atomic_u32_i32_buffer(&buffer_ty.kind) {
        return Err(LoweringError::custom(
            format!("{name} requires an Array<Atomic<u32|i32>, N> buffer, got {buffer_ty}"),
            expr.span,
            None,
        ));
    }

    let buffer_op = lower_expression(ctx, &args[0], None)?;
    let index_op = lower_expression(ctx, &args[1], None)?;
    let value_op = lower_expression(ctx, &args[2], None)?;
    let compare_expected = if op == GpuAtomicOp::CompareExchange {
        Some(Box::new(lower_expression(ctx, &args[3], None)?))
    } else {
        None
    };

    // The result is the old value (or the CAS result).
    let return_ty = resolve_type(ctx.type_checker, expr);
    let (target, ret_op) = if let Some(ref d) = dest {
        (d.clone(), Operand::Copy(d.clone()))
    } else {
        let temp = ctx.push_temp(return_ty, expr.span);
        (Place::new(temp), Operand::Copy(Place::new(temp)))
    };

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            target,
            Rvalue::AtomicOp {
                op,
                buffer: Box::new(buffer_op),
                index: Box::new(index_op),
                value: Box::new(value_op),
                compare_expected,
            },
        ),
        span: expr.span,
    });

    mark_atomic_capability(ctx);
    Ok(Some(ret_op))
}

fn try_lower_vector_builtin(
    ctx: &mut LoweringContext,
    name: &str,
    args: &[Expression],
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    // Vector builtins are dispatched by name and first-argument type, not module.
    // The type checker has already validated that these are vector operations.
    let intrinsic_opt = match name {
        "dot" => Some(MathIntrinsic::VecDot),
        "length" => Some(MathIntrinsic::VecLength),
        "normalize" => Some(MathIntrinsic::VecNormalize),
        "cross" => Some(MathIntrinsic::VecCross),
        "reflect" => Some(MathIntrinsic::VecReflect),
        "mix" => Some(MathIntrinsic::VecMix),
        _ => None,
    };

    let Some(intrinsic) = intrinsic_opt else {
        return Ok(None);
    };

    // Verify the first argument is a vector type (type checker already validates this)
    if args.is_empty() {
        return Ok(None);
    }

    let first_arg_ty = resolve_type(ctx.type_checker, &args[0]);
    if !matches!(&first_arg_ty.kind, TypeKind::Custom(name, _) if vec_dim(name).is_some()) {
        return Ok(None);
    }

    let mut arg_ops = Vec::with_capacity(args.len());
    for arg in args {
        arg_ops.push(lower_expression(ctx, arg, None)?);
    }

    let return_ty = resolve_type(ctx.type_checker, expr);
    let return_ty = gpu_math_return_type(ctx, args, return_ty, expr.span);
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
    let return_ty = gpu_math_return_type(ctx, args, return_ty, expr.span);
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
