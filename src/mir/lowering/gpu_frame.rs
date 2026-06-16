// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR lowering for `gpu frame <ident> in <range>` loops.
//!
//! A `gpu frame` loop is a variant of `gpu for` that synthesizes a kernel marked
//! with `is_frame_step=true` for animation drivers. The frame kernel is structured
//! as a `gpu for` kernel plus 11 leading frame-input uniform parameters (f0..f10),
//! which precede any ordinary scalar captures. This parameter ordering ensures the
//! WGSL `_Inputs` struct fields at offsets 0–40 are fixed for frame fields,
//! simplifying integration with the web-gpu driver.
//!
//! The frame input fields (time, dt, index, mouse_x, mouse_y, mouse_down, drag_dx,
//! drag_dy, wheel, clicked, double_clicked) are lowered as UniformBuffer parameters
//! and accessed by name-based lookup in member_expr. Deduplication with forall_gpu
//! kernel building logic is a future cleanup task.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::{AcceleratorTarget, Statement, StatementKind, VariableDeclaration};
use crate::ast::types::{frame_input_param_key, Type, TypeKind, FRAME_INPUT_FIELDS};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{
    BackendMetadata, BinOp, Body, Dimension, Discriminant, ExecutionModel, GpuBodyMetadata,
    LocalDecl, Operand, Place, Rvalue, StorageClass, Terminator, TerminatorKind,
};

use super::context::LoweringContext;
use super::forall_gpu;
use super::statement::lower_statement;

/// Lowers a single-pass `gpu frame` loop into a synthesized kernel + `GpuLaunch`.
///
/// This is a wrapper around `emit_frame_pass` for the single-pass case.
/// Creates a kernel marked with `is_frame_step=true` and injects frame inputs.
pub fn lower_gpu_frame(
    ctx: &mut LoweringContext,
    span: &Span,
    stmt_id: usize,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) -> Result<(), LoweringError> {
    let loop_var_name = decls[0].name.clone();

    let ExpressionKind::Range(start, Some(end), range_type) = &iterable.node else {
        return Err(LoweringError::unsupported_expression(
            "gpu frame: iterable must be a bounded numeric range like '0..n'".to_string(),
            *span,
        ));
    };

    let start_lit = forall_gpu::read_int_literal(start, *span)?;
    let is_literal_end = matches!(
        &end.node,
        ExpressionKind::Literal(crate::ast::literal::Literal::Integer(_))
    );

    let captures = forall_gpu::collect_capture_infos(ctx, body, &loop_var_name, *span)?;
    let uses_frame = detect_frame_usage(body);

    // Single-pass uses emit_frame_pass with pass_idx=0.
    emit_frame_pass(
        ctx,
        span,
        stmt_id,
        0,
        decls,
        start_lit,
        is_literal_end,
        end,
        range_type.clone(),
        &captures,
        body,
        uses_frame,
    )?;

    Ok(())
}

/// Lowers a `gpu frame` block (multi-pass form) into ordered frame passes.
///
/// Each child of the block MUST be a `Forall` statement. They are lowered
/// sequentially, each marked as a frame step with frame inputs injected.
/// Targets are not chained here; that's a future enhancement.
pub fn lower_gpu_frame_block(
    ctx: &mut LoweringContext,
    span: &Span,
    _stmt_id: usize,
    block: &Statement,
) -> Result<(), LoweringError> {
    // Extract the statements from the block
    let stmts = match &block.node {
        crate::ast::statement::StatementKind::Block(stmts) => stmts,
        _ => {
            return Err(LoweringError::unsupported_expression(
                "gpu frame block body must be a block statement".to_string(),
                *span,
            ));
        }
    };

    // Validate that all statements are gpu forall loops
    for stmt in stmts {
        match &stmt.node {
            StatementKind::Forall {
                device: AcceleratorTarget::Gpu,
                ..
            } => {}
            _ => {
                return Err(LoweringError::unsupported_expression(
                    "gpu frame block may only contain 'gpu forall' passes".to_string(),
                    stmt.span,
                ));
            }
        }
    }

    if stmts.is_empty() {
        return Err(LoweringError::unsupported_expression(
            "gpu frame block must contain at least one 'gpu forall' pass".to_string(),
            *span,
        ));
    }

    // Lower each pass as a frame step with frame inputs.
    for (pass_idx, pass_stmt) in stmts.iter().enumerate() {
        if let StatementKind::Forall {
            vars: decls,
            iterable,
            body,
            ..
        } = &pass_stmt.node
        {
            let loop_var_name = decls[0].name.clone();

            let ExpressionKind::Range(start, Some(end), range_type) = &iterable.node else {
                return Err(LoweringError::unsupported_expression(
                    "gpu frame: iterable must be a bounded numeric range like '0..n'".to_string(),
                    *span,
                ));
            };

            let start_lit = forall_gpu::read_int_literal(start, *span)?;
            let is_literal_end = matches!(
                &end.node,
                ExpressionKind::Literal(crate::ast::literal::Literal::Integer(_))
            );

            let captures = forall_gpu::collect_capture_infos(ctx, body, &loop_var_name, *span)?;
            let uses_frame = detect_frame_usage(body);

            // Use emit_frame_pass for each pass in the block.
            emit_frame_pass(
                ctx,
                span,
                pass_stmt.id,
                pass_idx,
                decls,
                start_lit,
                is_literal_end,
                end,
                range_type.clone(),
                &captures,
                body,
                uses_frame,
            )?;
        }
    }

    Ok(())
}

/// Emits a single frame pass kernel with frame inputs injected and is_frame_step=true.
///
/// This is the core reusable helper that both single-pass and multi-pass use.
/// It creates a kernel with a unique name based on frame_stmt_id and pass_idx,
/// marks it as a frame step, injects frame inputs, and emits a GpuLaunch.
#[allow(clippy::too_many_arguments)]
fn emit_frame_pass(
    ctx: &mut LoweringContext,
    span: &Span,
    frame_stmt_id: usize,
    pass_idx: usize,
    decls: &[VariableDeclaration],
    start_lit: i64,
    is_literal_end: bool,
    end: &Expression,
    range_type: crate::ast::RangeExpressionType,
    captures: &[forall_gpu::CaptureInfo],
    body: &Statement,
    uses_frame: bool,
) -> Result<(), LoweringError> {
    let loop_var_name = &decls[0].name;

    // Distinct kernel name to avoid runtime cache collision.
    let kernel_name = format!("miri_gpu_for_{}_{}", frame_stmt_id, pass_idx);

    if is_literal_end {
        let end_lit = forall_gpu::read_int_literal(end, *span)?;
        let length =
            forall_gpu::compute_range_length(start_lit, end_lit, range_type.clone(), *span)?;
        let kernel_body = build_frame_kernel_literal(
            ctx,
            captures,
            loop_var_name,
            start_lit,
            length,
            body,
            *span,
            uses_frame,
        )?;
        ctx.lambda_bodies.push(crate::mir::lambda::LambdaInfo {
            name: kernel_name.clone(),
            body: kernel_body,
            captures: Vec::new(),
        });
        emit_gpu_frame_launch_literal(ctx, &kernel_name, length, captures, *span, uses_frame);
    } else {
        let kernel_body = build_frame_kernel_runtime(
            ctx,
            captures,
            loop_var_name,
            start_lit,
            body,
            *span,
            uses_frame,
        )?;
        ctx.lambda_bodies.push(crate::mir::lambda::LambdaInfo {
            name: kernel_name.clone(),
            body: kernel_body,
            captures: Vec::new(),
        });
        emit_gpu_frame_launch_runtime(
            ctx,
            &kernel_name,
            start_lit,
            end,
            range_type.clone(),
            captures,
            *span,
            uses_frame,
        )?;
    }

    Ok(())
}

/// Helper to construct and register grid/block Dim3 locals with a literal grid-x value.
fn make_grid_block_locals(
    ctx: &mut LoweringContext,
    grid_x: u32,
    span: Span,
) -> (crate::mir::Local, crate::mir::Local) {
    let dim3_ty = Type::new(TypeKind::Custom("Dim3".to_string(), None), span);
    let one_op = forall_gpu::int_constant(1, span);
    let grid_x_op = forall_gpu::int_constant(i64::from(grid_x), span);
    let block_size_i64 = i64::from(forall_gpu::FORALL_GPU_BLOCK_SIZE);
    let block_x_op = forall_gpu::int_constant(block_size_i64, span);

    let grid_local = ctx.push_temp(dim3_ty.clone(), span);
    forall_gpu::push_assign(
        ctx,
        grid_local,
        Rvalue::Aggregate(
            crate::mir::AggregateKind::Struct(dim3_ty.clone()),
            vec![grid_x_op, one_op.clone(), one_op.clone()],
        ),
        span,
    );
    let block_local = ctx.push_temp(dim3_ty.clone(), span);
    forall_gpu::push_assign(
        ctx,
        block_local,
        Rvalue::Aggregate(
            crate::mir::AggregateKind::Struct(dim3_ty.clone()),
            vec![block_x_op, one_op.clone(), one_op],
        ),
        span,
    );
    (grid_local, block_local)
}

/// Helper to emit bounds-check loop for literal-bound gpu frame kernel.
fn emit_literal_frame_bounds_check(
    ctx: &mut LoweringContext,
    loop_var_name: &str,
    start: i64,
    length: i64,
    body: &Statement,
    span: Span,
) -> Result<(), LoweringError> {
    let i64_ty = Type::new(TypeKind::Int, span);
    let thread_int = forall_gpu::compute_thread_index(ctx, Dimension::X, span);

    let loop_local = ctx.push_local(loop_var_name.to_string(), i64_ty, span);
    forall_gpu::push_assign(
        ctx,
        loop_local,
        Rvalue::BinaryOp(
            BinOp::Add,
            Box::new(Operand::Copy(Place::new(thread_int))),
            Box::new(forall_gpu::int_constant(start, span)),
        ),
        span,
    );

    let cond_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    let limit = start
        .checked_add(length)
        .ok_or_else(|| forall_gpu::bounds_overflow_err(span))?;
    forall_gpu::push_assign(
        ctx,
        cond_local,
        Rvalue::BinaryOp(
            BinOp::Lt,
            Box::new(Operand::Copy(Place::new(loop_local))),
            Box::new(forall_gpu::int_constant(limit, span)),
        ),
        span,
    );

    let body_bb = ctx.new_basic_block();
    let exit_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(cond_local)),
            targets: vec![(Discriminant::bool_true(), body_bb)],
            otherwise: exit_bb,
        },
        span,
    ));

    ctx.set_current_block(body_bb);
    lower_statement(ctx, body)?;
    if ctx.body.basic_blocks[ctx.current_block.0]
        .terminator
        .is_none()
    {
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto { target: exit_bb },
            span,
        ));
    }

    ctx.set_current_block(exit_bb);
    ctx.set_terminator(Terminator::new(TerminatorKind::Return, span));
    Ok(())
}

/// Helper to compute bounds limit operand for a runtime range.
fn compute_bounds_limit(
    ctx: &mut LoweringContext,
    end_op: Operand,
    range_type: crate::ast::RangeExpressionType,
    span: Span,
) -> Result<Operand, LoweringError> {
    let i64_ty = Type::new(TypeKind::Int, span);
    match range_type {
        crate::ast::RangeExpressionType::Exclusive => Ok(end_op),
        crate::ast::RangeExpressionType::Inclusive => {
            let limit_op = ctx.push_temp(i64_ty, span);
            forall_gpu::push_assign(
                ctx,
                limit_op,
                Rvalue::BinaryOp(
                    BinOp::Add,
                    Box::new(end_op),
                    Box::new(forall_gpu::int_constant(1, span)),
                ),
                span,
            );
            Ok(Operand::Copy(Place::new(limit_op)))
        }
        _ => Err(LoweringError::unsupported_expression(
            "gpu frame: iterable-object ranges are not supported".to_string(),
            span,
        )),
    }
}

/// Helper to construct grid/block Dim3 locals where grid-x comes from a computed local.
fn make_grid_block_locals_from_local(
    ctx: &mut LoweringContext,
    grid_x_local: crate::mir::Local,
    span: Span,
) -> (crate::mir::Local, crate::mir::Local) {
    let dim3_ty = Type::new(TypeKind::Custom("Dim3".to_string(), None), span);
    let one_op = forall_gpu::int_constant(1, span);
    let block_size_i64 = i64::from(forall_gpu::FORALL_GPU_BLOCK_SIZE);
    let block_x_op = forall_gpu::int_constant(block_size_i64, span);

    let grid_local = ctx.push_temp(dim3_ty.clone(), span);
    forall_gpu::push_assign(
        ctx,
        grid_local,
        Rvalue::Aggregate(
            crate::mir::AggregateKind::Struct(dim3_ty.clone()),
            vec![
                Operand::Copy(Place::new(grid_x_local)),
                one_op.clone(),
                one_op.clone(),
            ],
        ),
        span,
    );
    let block_local = ctx.push_temp(dim3_ty.clone(), span);
    forall_gpu::push_assign(
        ctx,
        block_local,
        Rvalue::Aggregate(
            crate::mir::AggregateKind::Struct(dim3_ty),
            vec![block_x_op, one_op.clone(), one_op],
        ),
        span,
    );
    (grid_local, block_local)
}

fn emit_gpu_frame_launch_literal(
    ctx: &mut LoweringContext,
    kernel_name: &str,
    length: i64,
    captures: &[forall_gpu::CaptureInfo],
    span: Span,
    uses_frame: bool,
) {
    let void_ty = Type::new(TypeKind::Void, span);
    let grid_x = forall_gpu::literal_grid_x(length);
    let (grid_local, block_local) = make_grid_block_locals(ctx, grid_x, span);

    let kernel_op = Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: crate::ast::literal::Literal::Identifier(kernel_name.to_string()),
    }));

    let (buffer_captures, scalar_captures): (Vec<_>, Vec<_>) =
        captures.iter().partition(|c| !c.is_scalar);

    let buffer_ops: Vec<Operand> = buffer_captures
        .iter()
        .map(|c| Operand::Copy(Place::new(c.outer_local)))
        .collect();
    let scalar_ops: Vec<Operand> = scalar_captures
        .iter()
        .map(|c| Operand::Copy(Place::new(c.outer_local)))
        .collect();

    let arg_handles: Vec<Option<crate::mir::body::DeviceHandleId>> = buffer_captures
        .iter()
        .map(|c| ctx.body.local_decls[c.outer_local.0].device_handle)
        .collect();
    let arg_int_narrow: Vec<bool> = buffer_captures
        .iter()
        .map(|c| forall_gpu::needs_int_narrowing(&c.ty))
        .collect();

    let mut all_scalar_ops = Vec::new();
    if uses_frame {
        all_scalar_ops.extend(create_frame_input_zeros(ctx, span));
    }
    all_scalar_ops.extend(scalar_ops);

    let dest_local = ctx.push_temp(void_ty, span);
    let after_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::GpuLaunch {
            kernel: kernel_op,
            grid: Operand::Copy(Place::new(grid_local)),
            block: Operand::Copy(Place::new(block_local)),
            args: buffer_ops,
            arg_handles,
            arg_read_only: buffer_captures.iter().map(|c| !c.is_written).collect(),
            arg_int_narrow,
            scalar_args: all_scalar_ops,
            uniform_bound_x: None,
            uniform_bound_y: None,
            destination: Place::new(dest_local),
            target: Some(after_bb),
        },
        span,
    ));
    ctx.set_current_block(after_bb);
}

#[allow(clippy::too_many_arguments)]
fn emit_gpu_frame_launch_runtime(
    ctx: &mut LoweringContext,
    kernel_name: &str,
    start: i64,
    end: &Expression,
    range_type: crate::ast::RangeExpressionType,
    captures: &[forall_gpu::CaptureInfo],
    span: Span,
    uses_frame: bool,
) -> Result<(), LoweringError> {
    let end_op = super::expression::lower_expression(ctx, end, None)?;

    let void_ty = Type::new(TypeKind::Void, span);

    let clamped_length_local = forall_gpu::compute_clamped_length(ctx, end_op.clone(), start, span);
    let grid_x_local = forall_gpu::compute_grid_size(ctx, clamped_length_local, span);
    let grid_x = crate::mir::Local(grid_x_local.0);
    let (grid_local, block_local) = make_grid_block_locals_from_local(ctx, grid_x, span);

    let kernel_op = Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: crate::ast::literal::Literal::Identifier(kernel_name.to_string()),
    }));

    let (buffer_captures, scalar_captures): (Vec<_>, Vec<_>) =
        captures.iter().partition(|c| !c.is_scalar);

    let buffer_ops: Vec<Operand> = buffer_captures
        .iter()
        .map(|c| Operand::Copy(Place::new(c.outer_local)))
        .collect();
    let scalar_ops: Vec<Operand> = scalar_captures
        .iter()
        .map(|c| Operand::Copy(Place::new(c.outer_local)))
        .collect();

    let arg_handles: Vec<Option<crate::mir::body::DeviceHandleId>> = buffer_captures
        .iter()
        .map(|c| ctx.body.local_decls[c.outer_local.0].device_handle)
        .collect();
    let arg_int_narrow: Vec<bool> = buffer_captures
        .iter()
        .map(|c| forall_gpu::needs_int_narrowing(&c.ty))
        .collect();

    let mut all_scalar_ops = Vec::new();
    if uses_frame {
        all_scalar_ops.extend(create_frame_input_zeros(ctx, span));
    }
    all_scalar_ops.extend(scalar_ops);

    let bounds_limit_op = compute_bounds_limit(ctx, end_op, range_type, span)?;

    let dest_local = ctx.push_temp(void_ty, span);
    let after_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::GpuLaunch {
            kernel: kernel_op,
            grid: Operand::Copy(Place::new(grid_local)),
            block: Operand::Copy(Place::new(block_local)),
            args: buffer_ops,
            arg_handles,
            arg_read_only: buffer_captures.iter().map(|c| !c.is_written).collect(),
            arg_int_narrow,
            scalar_args: all_scalar_ops,
            uniform_bound_x: Some(Box::new(bounds_limit_op)),
            uniform_bound_y: None,
            destination: Place::new(dest_local),
            target: Some(after_bb),
        },
        span,
    ));
    ctx.set_current_block(after_bb);
    Ok(())
}

fn create_frame_input_zeros(ctx: &mut LoweringContext, span: Span) -> Vec<Operand> {
    vec![
        create_zero_local(ctx, Type::new(TypeKind::F32, span), span),
        create_zero_local(ctx, Type::new(TypeKind::F32, span), span),
        create_zero_local(ctx, Type::new(TypeKind::Int, span), span),
        create_zero_local(ctx, Type::new(TypeKind::F32, span), span),
        create_zero_local(ctx, Type::new(TypeKind::F32, span), span),
        create_zero_local(ctx, Type::new(TypeKind::Boolean, span), span),
        create_zero_local(ctx, Type::new(TypeKind::F32, span), span),
        create_zero_local(ctx, Type::new(TypeKind::F32, span), span),
        create_zero_local(ctx, Type::new(TypeKind::F32, span), span),
        create_zero_local(ctx, Type::new(TypeKind::Boolean, span), span),
        create_zero_local(ctx, Type::new(TypeKind::Boolean, span), span),
    ]
}

fn create_zero_local(ctx: &mut LoweringContext, ty: Type, span: Span) -> Operand {
    let zero = if matches!(ty.kind, TypeKind::F32) {
        Operand::Constant(Box::new(crate::mir::Constant {
            span,
            ty: ty.clone(),
            literal: crate::ast::literal::Literal::Float(crate::ast::literal::FloatLiteral::F32(
                0.0_f32.to_bits(),
            )),
        }))
    } else if matches!(ty.kind, TypeKind::Boolean) {
        Operand::Constant(Box::new(crate::mir::Constant {
            span,
            ty: ty.clone(),
            literal: crate::ast::literal::Literal::Integer(
                crate::ast::literal::IntegerLiteral::U32(0),
            ),
        }))
    } else {
        Operand::Constant(Box::new(crate::mir::Constant {
            span,
            ty: ty.clone(),
            literal: crate::ast::literal::Literal::Integer(
                crate::ast::literal::IntegerLiteral::I32(0),
            ),
        }))
    };
    let temp = ctx.push_temp(ty, span);
    forall_gpu::push_assign(ctx, temp, Rvalue::Use(zero), span);
    Operand::Copy(Place::new(temp))
}

fn detect_frame_usage(stmt: &Statement) -> bool {
    use crate::ast::statement::StatementKind;
    match &stmt.node {
        StatementKind::Block(stmts) => stmts.iter().any(detect_frame_usage),
        StatementKind::Expression(expr) => detect_frame_usage_expr(expr),
        StatementKind::If(cond, then_branch, else_branch, _) => {
            detect_frame_usage_expr(cond)
                || detect_frame_usage(then_branch)
                || else_branch.as_ref().is_some_and(|b| detect_frame_usage(b))
        }
        StatementKind::While(cond, body, _) => {
            detect_frame_usage_expr(cond) || detect_frame_usage(body)
        }
        StatementKind::For(_, iterable, body) => {
            detect_frame_usage_expr(iterable) || detect_frame_usage(body)
        }
        StatementKind::Forall { iterable, body, .. } => {
            detect_frame_usage_expr(iterable) || detect_frame_usage(body)
        }
        StatementKind::GpuFrame(_, iterable, body) => {
            detect_frame_usage_expr(iterable) || detect_frame_usage(body)
        }
        StatementKind::Variable(decls, _) => decls.iter().any(|d| {
            d.initializer
                .as_ref()
                .is_some_and(|e| detect_frame_usage_expr(e))
        }),
        _ => false,
    }
}

fn detect_frame_usage_expr(expr: &Expression) -> bool {
    use crate::ast::expression::ExpressionKind;
    match &expr.node {
        ExpressionKind::Member(obj, prop) => {
            if let ExpressionKind::Identifier(name, _) = &obj.node {
                if name == "frame" {
                    return true;
                }
            }
            detect_frame_usage_expr(obj) || detect_frame_usage_expr(prop)
        }
        ExpressionKind::Identifier(_, _)
        | ExpressionKind::Literal(_)
        | ExpressionKind::Super
        | ExpressionKind::Type(_, _)
        | ExpressionKind::GenericType(_, _, _)
        | ExpressionKind::StructMember(_, _)
        | ExpressionKind::List(_)
        | ExpressionKind::Array(_, _)
        | ExpressionKind::Map(_)
        | ExpressionKind::Set(_)
        | ExpressionKind::Tuple(_)
        | ExpressionKind::Match(_, _)
        | ExpressionKind::Block(_, _) => false,
        ExpressionKind::Index(base, idx) => {
            detect_frame_usage_expr(base) || detect_frame_usage_expr(idx)
        }
        ExpressionKind::Binary(left, _, right) => {
            detect_frame_usage_expr(left) || detect_frame_usage_expr(right)
        }
        ExpressionKind::Logical(left, _, right) => {
            detect_frame_usage_expr(left) || detect_frame_usage_expr(right)
        }
        ExpressionKind::Unary(_, arg) => detect_frame_usage_expr(arg),
        ExpressionKind::Assignment(lhs, _, rhs) => {
            use crate::ast::expression::LeftHandSideExpression;
            let lhs_frame = match &**lhs {
                LeftHandSideExpression::Identifier(e)
                | LeftHandSideExpression::Member(e)
                | LeftHandSideExpression::Index(e) => detect_frame_usage_expr(e),
            };
            lhs_frame || detect_frame_usage_expr(rhs)
        }
        ExpressionKind::Call(func, args) => {
            detect_frame_usage_expr(func) || args.iter().any(detect_frame_usage_expr)
        }
        ExpressionKind::Conditional(cond, then_expr, else_expr, _) => {
            detect_frame_usage_expr(cond)
                || detect_frame_usage_expr(then_expr)
                || else_expr
                    .as_ref()
                    .is_some_and(|e| detect_frame_usage_expr(e))
        }
        ExpressionKind::Range(start, end, _) => {
            detect_frame_usage_expr(start)
                || end.as_ref().is_some_and(|e| detect_frame_usage_expr(e))
        }
        ExpressionKind::Lambda(_)
        | ExpressionKind::TypeDeclaration(_, _, _, _)
        | ExpressionKind::ImportPath(_, _)
        | ExpressionKind::Cast(_, _)
        | ExpressionKind::Guard(_, _)
        | ExpressionKind::FormattedString(_)
        | ExpressionKind::EnumValue(_, _)
        | ExpressionKind::NamedArgument(_, _) => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_frame_kernel_literal(
    parent: &mut LoweringContext,
    captures: &[forall_gpu::CaptureInfo],
    loop_var_name: &str,
    start: i64,
    length: i64,
    body: &Statement,
    span: Span,
    uses_frame: bool,
) -> Result<Body, LoweringError> {
    let (buffer_captures, scalar_captures): (Vec<_>, Vec<_>) =
        captures.iter().partition(|c| !c.is_scalar);

    let frame_param_count = if uses_frame { 11 } else { 0 };
    let total_params = buffer_captures.len() + scalar_captures.len() + frame_param_count;
    let mut kernel = Body::new(total_params, span, ExecutionModel::GpuKernel);
    kernel
        .local_decls
        .push(LocalDecl::new(Type::new(TypeKind::Void, span), span));

    let grid_x = forall_gpu::literal_grid_x(length);
    kernel.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some([forall_gpu::FORALL_GPU_BLOCK_SIZE, 1, 1]),
        grid_size: Some([grid_x, 1, 1]),
        required_capabilities: Vec::new(),
        is_frame_step: true,
    }));

    let mut out_params: Vec<bool> = buffer_captures.iter().map(|c| c.is_written).collect();
    out_params.extend(std::iter::repeat_n(false, frame_param_count));
    out_params.extend(scalar_captures.iter().map(|_| false));
    kernel.out_params = out_params;

    let mut ctx = LoweringContext::new(kernel, parent.type_checker, parent.is_release);

    for cap in buffer_captures {
        let local = ctx.push_param(cap.name.clone(), cap.ty.clone(), span);
        ctx.body.local_decls[local.0].storage_class = StorageClass::GpuGlobal;
    }

    if uses_frame {
        for (idx, field_def) in FRAME_INPUT_FIELDS.iter().enumerate() {
            let ty = match field_def.kind {
                crate::ast::types::FrameFieldKind::F32 => Type::new(TypeKind::F32, span),
                crate::ast::types::FrameFieldKind::Int => Type::new(TypeKind::Int, span),
                crate::ast::types::FrameFieldKind::Bool => Type::new(TypeKind::Boolean, span),
            };
            let field_local = ctx.push_param(format!("f{}", idx), ty, span);
            ctx.body.local_decls[field_local.0].storage_class = StorageClass::UniformBuffer;
            // Register under reserved key to prevent user-variable shadowing
            ctx.variable_map
                .insert(frame_input_param_key(idx).into(), field_local);
        }
    }

    for cap in scalar_captures {
        let local = ctx.push_param(cap.name.clone(), cap.ty.clone(), span);
        ctx.body.local_decls[local.0].storage_class = StorageClass::UniformBuffer;
    }

    emit_literal_frame_bounds_check(&mut ctx, loop_var_name, start, length, body, span)?;
    Ok(ctx.body)
}

fn build_frame_kernel_runtime(
    parent: &mut LoweringContext,
    captures: &[forall_gpu::CaptureInfo],
    loop_var_name: &str,
    start: i64,
    body: &Statement,
    span: Span,
    uses_frame: bool,
) -> Result<Body, LoweringError> {
    let (buffer_captures, scalar_captures): (Vec<_>, Vec<_>) =
        captures.iter().partition(|c| !c.is_scalar);

    let frame_param_count = if uses_frame { 11 } else { 0 };
    let arg_count = captures.len() + 1 + frame_param_count;
    let mut kernel = Body::new(arg_count, span, ExecutionModel::GpuKernel);
    kernel
        .local_decls
        .push(LocalDecl::new(Type::new(TypeKind::Void, span), span));
    kernel.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some([forall_gpu::FORALL_GPU_BLOCK_SIZE, 1, 1]),
        grid_size: None,
        required_capabilities: Vec::new(),
        is_frame_step: true,
    }));

    let mut out_params: Vec<bool> = buffer_captures.iter().map(|c| c.is_written).collect();
    out_params.extend(std::iter::repeat_n(false, frame_param_count));
    out_params.push(false);
    out_params.extend(scalar_captures.iter().map(|_| false));
    kernel.out_params = out_params;

    let mut ctx = LoweringContext::new(kernel, parent.type_checker, parent.is_release);

    for cap in buffer_captures {
        let local = ctx.push_param(cap.name.clone(), cap.ty.clone(), span);
        ctx.body.local_decls[local.0].storage_class = StorageClass::GpuGlobal;
    }

    if uses_frame {
        for (idx, field_def) in FRAME_INPUT_FIELDS.iter().enumerate() {
            let ty = match field_def.kind {
                crate::ast::types::FrameFieldKind::F32 => Type::new(TypeKind::F32, span),
                crate::ast::types::FrameFieldKind::Int => Type::new(TypeKind::Int, span),
                crate::ast::types::FrameFieldKind::Bool => Type::new(TypeKind::Boolean, span),
            };
            let field_local = ctx.push_param(format!("f{}", idx), ty, span);
            ctx.body.local_decls[field_local.0].storage_class = StorageClass::UniformBuffer;
            // Register under reserved key to prevent user-variable shadowing
            ctx.variable_map
                .insert(frame_input_param_key(idx).into(), field_local);
        }
    }

    let i64_ty = Type::new(TypeKind::Int, span);
    let uniform_param = ctx.push_param("_uniform_bound".to_string(), i64_ty.clone(), span);
    ctx.body.local_decls[uniform_param.0].storage_class = StorageClass::UniformBuffer;

    for cap in scalar_captures {
        let local = ctx.push_param(cap.name.clone(), cap.ty.clone(), span);
        ctx.body.local_decls[local.0].storage_class = StorageClass::UniformBuffer;
    }

    let thread_int = forall_gpu::compute_thread_index(&mut ctx, Dimension::X, span);

    let loop_local = ctx.push_local(loop_var_name.to_string(), i64_ty.clone(), span);
    forall_gpu::push_assign(
        &mut ctx,
        loop_local,
        Rvalue::BinaryOp(
            BinOp::Add,
            Box::new(Operand::Copy(Place::new(thread_int))),
            Box::new(forall_gpu::int_constant(start, span)),
        ),
        span,
    );

    forall_gpu::emit_bounds_check_loop(&mut ctx, loop_local, uniform_param, body, span)?;

    Ok(ctx.body)
}
