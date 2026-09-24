// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR lowering for `gpu frame <ident> in <range>` loops.
//!
//! A `gpu frame` loop is a variant of `forall` that synthesizes a kernel marked
//! with `is_frame_step=true` for animation drivers. The frame kernel is structured
//! as a `forall` kernel plus the leading frame-input uniform parameters (f0..fN),
//! which precede any ordinary scalar captures. This parameter ordering ensures the
//! WGSL `_Inputs` struct fields (one 4-byte slot each from offset 0) are fixed for frame fields,
//! simplifying integration with the web-gpu driver.
//!
//! The frame input fields (time, dt, index, mouse_x, mouse_y, mouse_down, drag_dx,
//! drag_dy, wheel, clicked, double_clicked) are lowered as UniformBuffer parameters
//! and accessed by name-based lookup in member_expr.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::{Statement, StatementKind, VariableDeclaration};
use crate::ast::types::{
    frame_input_param_key, FrameFieldKind, Type, TypeKind, FRAME_INPUT_FIELDS,
};
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::backend::BackendConfig;
use crate::mir::body::LaunchUniform;
use crate::mir::{
    BackendMetadata, BinOp, Body, Dimension, ExecutionModel, GpuBodyMetadata, GpuLaunchArgs,
    LocalDecl, Operand, Place, Rvalue, StorageClass, Terminator, TerminatorKind,
};

use super::context::LoweringContext;
use super::forall_gpu;

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
    let ExpressionKind::Range(start, Some(end), range_type) = &iterable.node else {
        return Err(LoweringError::unsupported_expression(
            "gpu frame: iterable must be a bounded numeric range like '0..n'".to_string(),
            *span,
        ));
    };

    // Fold const-valued bounds to literals so `0..CONST` dispatches a real grid
    // instead of the runtime `_bound` uniform (which collides with `_Inputs`).
    let start = forall_gpu::fold_const_bound(ctx, start);
    let end = forall_gpu::fold_const_bound(ctx, end);
    let start_lit = forall_gpu::read_int_literal(&start, *span)?;
    let is_literal_end = matches!(
        &end.node,
        ExpressionKind::Literal(crate::ast::literal::Literal::Integer(_))
    );

    let captures = forall_gpu::collect_capture_infos(ctx, body, decls, *span)?;
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
        &end,
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

    // Flatten the block into an ordered list of `gpu forall` passes, expanding
    // any literal-count `for _ in 0..k` repeat into `k` sequential copies.
    let passes = crate::ast::gpu_frame_passes::flatten_frame_passes(stmts)
        .map_err(|(msg, sp)| LoweringError::unsupported_expression(msg, sp))?;

    if passes.is_empty() {
        return Err(LoweringError::unsupported_expression(
            "gpu frame block must contain at least one 'gpu forall' pass".to_string(),
            *span,
        ));
    }

    // Lower each pass as a frame step with frame inputs.
    for (pass_idx, pass_stmt) in passes.iter().enumerate() {
        if let StatementKind::Forall {
            vars: decls,
            iterable,
            body,
            ..
        } = &pass_stmt.node
        {
            let ExpressionKind::Range(start, Some(end), range_type) = &iterable.node else {
                return Err(LoweringError::unsupported_expression(
                    "gpu frame: iterable must be a bounded numeric range like '0..n'".to_string(),
                    *span,
                ));
            };

            // Fold const-valued bounds to literals so `0..CONST` dispatches a real
            // grid rather than the runtime `_bound` uniform, which would collide
            // with the frame `_Inputs` binding and abort the launch.
            let start = forall_gpu::fold_const_bound(ctx, start);
            let end = forall_gpu::fold_const_bound(ctx, end);
            let start_lit = forall_gpu::read_int_literal(&start, *span)?;
            let is_literal_end = matches!(
                &end.node,
                ExpressionKind::Literal(crate::ast::literal::Literal::Integer(_))
            );

            let captures = forall_gpu::collect_capture_infos(ctx, body, decls, *span)?;
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
                &end,
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

    // Distinct kernel name to avoid runtime cache collision. Every pass of one
    // frame statement shares its compilation-local index; `pass_idx` keeps the
    // passes distinct.
    let kernel_name = format!(
        "miri_gpu_for_{}_{}",
        ctx.kernel_index(frame_stmt_id),
        pass_idx
    );

    if is_literal_end {
        let range = literal_frame_range(loop_var_name, start_lit, end, range_type.clone(), *span)?;
        let kernel_body =
            build_frame_kernel_literal(ctx, captures, &range, body, *span, uses_frame)?;
        ctx.lambda_bodies.push(crate::mir::lambda::LambdaInfo {
            name: kernel_name.clone(),
            body: kernel_body,
            captures: Vec::new(),
        });
        emit_gpu_frame_launch_literal(ctx, &kernel_name, range.grid, captures, *span, uses_frame)?;
    } else {
        let bound = forall_gpu::AxisBound::Runtime(end.clone(), range_type.clone());
        let kernel_body = build_frame_kernel_runtime(
            ctx,
            captures,
            &frame_axis(loop_var_name, start_lit, bound),
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

/// Builds the grid and block `Dim3` locals of a 1-D frame launch: `grid` per
/// axis, and `block_size` threads along x.
fn make_grid_block_locals(
    ctx: &mut LoweringContext,
    grid: [Operand; 3],
    block_size: u32,
    span: Span,
) -> (crate::mir::Local, crate::mir::Local) {
    let dim3_ty = Type::new(TypeKind::Custom("Dim3".to_string(), None), span);
    let one = || forall_gpu::int_constant(1, span);
    let block_x = forall_gpu::int_constant(i64::from(block_size), span);

    let grid_local = ctx.push_temp(dim3_ty.clone(), span);
    forall_gpu::push_assign(
        ctx,
        grid_local,
        Rvalue::Aggregate(
            crate::mir::AggregateKind::Struct(dim3_ty.clone()),
            grid.into(),
        ),
        span,
    );
    let block_local = ctx.push_temp(dim3_ty.clone(), span);
    forall_gpu::push_assign(
        ctx,
        block_local,
        Rvalue::Aggregate(
            crate::mir::AggregateKind::Struct(dim3_ty),
            vec![block_x, one(), one()],
        ),
        span,
    );
    (grid_local, block_local)
}

/// The loop axis of a literal-bound frame pass and the grid it dispatches.
#[derive(Debug, Clone)]
struct FrameRange {
    axis: forall_gpu::AxisSpec,
    grid: [u32; 3],
}

/// The axis and dispatch grid of a frame pass binding `name` over
/// `start..end` with a literal `end`.
fn literal_frame_range(
    name: &str,
    start: i64,
    end: &Expression,
    range_type: crate::ast::RangeExpressionType,
    span: Span,
) -> Result<FrameRange, LoweringError> {
    let end = forall_gpu::read_int_literal(end, span)?;
    let length = forall_gpu::compute_range_length(start, end, range_type.clone(), span)?;
    let block_size = BackendConfig::WEB_GPU.block_size(1)[0];
    let grid = forall_gpu::literal_grid_1d(length, block_size, span)?;
    let bound = forall_gpu::AxisBound::Literal(end, range_type);
    Ok(FrameRange {
        axis: frame_axis(name, start, bound),
        grid,
    })
}

/// The single x axis a frame pass binding `name` iterates, counting from the
/// literal `start` to `bound`.
fn frame_axis(name: &str, start: i64, bound: forall_gpu::AxisBound) -> forall_gpu::AxisSpec {
    forall_gpu::AxisSpec {
        name: name.to_string(),
        start: forall_gpu::AxisStart::Literal(start),
        dimension: Dimension::X,
        bound,
    }
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

fn emit_gpu_frame_launch_literal(
    ctx: &mut LoweringContext,
    kernel_name: &str,
    grid: [u32; 3],
    captures: &[forall_gpu::CaptureInfo],
    span: Span,
    uses_frame: bool,
) -> Result<(), LoweringError> {
    let void_ty = Type::new(TypeKind::Void, span);
    let block_size = BackendConfig::WEB_GPU.block_size(1)[0];
    let grid_ops = grid.map(|axis| forall_gpu::int_constant(i64::from(axis), span));
    let (grid_local, block_local) = make_grid_block_locals(ctx, grid_ops, block_size, span);

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
    let arg_read_only: Vec<bool> = buffer_captures.iter().map(|c| !c.is_written).collect();
    let arg_int_narrow: Vec<bool> = buffer_captures
        .iter()
        .map(|c| forall_gpu::needs_wire_conversion(&c.ty))
        .collect();
    let launch_args = GpuLaunchArgs::new(buffer_ops, arg_handles, arg_read_only, arg_int_narrow)
        .map_err(|e| {
            LoweringError::internal(DiagnosticCode::MirGpuLaunchMetadataMismatch, e, span)
        })?;

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
            launch_args,
            scalar_args: all_scalar_ops,
            uniform_bound_x: None,
            uniform_bound_y: None,
            uniform_bound_z: None,
            uniform_start_x: None,
            uniform_start_y: None,
            uniform_start_z: None,
            destination: Place::new(dest_local),
            target: Some(after_bb),
        },
        span,
    ));
    ctx.set_current_block(after_bb);
    // The grid and block dimensions are allocations of their own, read by the
    // launch and dead once it returns.
    ctx.emit_temp_drop(grid_local, 0, span);
    ctx.emit_temp_drop(block_local, 0, span);
    Ok(())
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
    let block_size = BackendConfig::WEB_GPU.block_size(1)[0];
    let grid_ops = runtime_frame_grid(ctx, start, &end_op, block_size, span);
    let (grid_local, block_local) = make_grid_block_locals(ctx, grid_ops, block_size, span);

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
    let arg_read_only: Vec<bool> = buffer_captures.iter().map(|c| !c.is_written).collect();
    let arg_int_narrow: Vec<bool> = buffer_captures
        .iter()
        .map(|c| forall_gpu::needs_wire_conversion(&c.ty))
        .collect();
    let launch_args = GpuLaunchArgs::new(buffer_ops, arg_handles, arg_read_only, arg_int_narrow)
        .map_err(|e| {
            LoweringError::internal(DiagnosticCode::MirGpuLaunchMetadataMismatch, e, span)
        })?;

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
            launch_args,
            scalar_args: all_scalar_ops,
            uniform_bound_x: Some(Box::new(bounds_limit_op)),
            uniform_bound_y: None,
            uniform_bound_z: None,
            uniform_start_x: None,
            uniform_start_y: None,
            uniform_start_z: None,
            destination: Place::new(dest_local),
            target: Some(after_bb),
        },
        span,
    ));
    ctx.set_current_block(after_bb);
    // The grid and block dimensions are allocations of their own, read by the
    // launch and dead once it returns.
    ctx.emit_temp_drop(grid_local, 0, span);
    ctx.emit_temp_drop(block_local, 0, span);
    Ok(())
}

/// The grid of a frame pass over `start..end` with a runtime `end`, computed on
/// the host and spilled past one grid axis when it must be.
fn runtime_frame_grid(
    ctx: &mut LoweringContext,
    start: i64,
    end_op: &Operand,
    block_size: u32,
    span: Span,
) -> [Operand; 3] {
    let start_op = forall_gpu::int_constant(start, span);
    let length = forall_gpu::compute_clamped_length(ctx, end_op.clone(), start_op, span);
    let workgroups = forall_gpu::compute_grid_size(ctx, length, block_size, span);
    let (columns, rows) = forall_gpu::spill_runtime_grid(ctx, workgroups, span);
    [
        Operand::Copy(Place::new(columns)),
        Operand::Copy(Place::new(rows)),
        forall_gpu::int_constant(1, span),
    ]
}

fn create_frame_input_zeros(ctx: &mut LoweringContext, span: Span) -> Vec<Operand> {
    FRAME_INPUT_FIELDS
        .iter()
        .map(|field| {
            let kind = match field.kind {
                FrameFieldKind::F32 => TypeKind::F32,
                FrameFieldKind::Int => TypeKind::Int,
                FrameFieldKind::Bool => TypeKind::Boolean,
            };
            create_zero_local(ctx, Type::new(kind, span), span)
        })
        .collect()
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

/// Number of leading frame-input uniform parameters a kernel carries, derived
/// from the single source of truth [`FRAME_INPUT_FIELDS`] so adding a field
/// keeps the parameter count and the field-registration loop in lockstep.
fn frame_uniform_param_count(uses_frame: bool) -> usize {
    if uses_frame {
        FRAME_INPUT_FIELDS.len()
    } else {
        0
    }
}

fn detect_frame_usage(stmt: &Statement) -> bool {
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
        StatementKind::GpuFrameBlock(body) => detect_frame_usage(body),
        StatementKind::Variable(decls, _) => decls.iter().any(|d| {
            d.initializer
                .as_ref()
                .is_some_and(|e| detect_frame_usage_expr(e))
        }),
        StatementKind::Return(expr) => expr.as_ref().is_some_and(|e| detect_frame_usage_expr(e)),
        // Statements that carry no expression which can reference `frame`.
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Use(_, _)
        | StatementKind::Type(_, _)
        | StatementKind::Enum(_, _, _, _, _, _)
        | StatementKind::Struct(_, _, _, _, _, _)
        | StatementKind::Class(_)
        | StatementKind::Trait(_, _, _, _, _)
        | StatementKind::FunctionDeclaration(_)
        | StatementKind::RuntimeFunctionDeclaration(_, _, _, _)
        | StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => false,
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
        | ExpressionKind::Tuple(_) => false,
        ExpressionKind::Match(scrutinee, branches) => {
            detect_frame_usage_expr(scrutinee) || branches.iter().any(detect_frame_usage_branch)
        }
        ExpressionKind::Block(stmts, value) => {
            stmts.iter().any(detect_frame_usage) || detect_frame_usage_expr(value)
        }
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
        // Expressions that wrap a subexpression which can reference `frame`:
        // recurse so a `frame.*` read nested inside them still injects the frame
        // inputs (e.g. `(frame.time * 2.0) as f32`, `f"{frame.dt}"`).
        ExpressionKind::Cast(value, _ty) => detect_frame_usage_expr(value),
        ExpressionKind::Guard(_, arg) => detect_frame_usage_expr(arg),
        ExpressionKind::FormattedString(parts) => parts.iter().any(detect_frame_usage_expr),
        ExpressionKind::NamedArgument(_, arg) => detect_frame_usage_expr(arg),
        ExpressionKind::Lambda(_)
        | ExpressionKind::TypeDeclaration(_, _, _, _)
        | ExpressionKind::ImportPath(_, _)
        | ExpressionKind::EnumValue(_, _) => false,
    }
}

/// Whether a `match` branch reads `frame` in its guard or its body.
fn detect_frame_usage_branch(branch: &crate::ast::pattern::MatchBranch) -> bool {
    let guard_reads_frame = branch
        .guard
        .as_ref()
        .is_some_and(|guard| detect_frame_usage_expr(guard));
    guard_reads_frame || detect_frame_usage(&branch.body)
}

fn build_frame_kernel_literal(
    parent: &mut LoweringContext,
    captures: &[forall_gpu::CaptureInfo],
    range: &FrameRange,
    body: &Statement,
    span: Span,
    uses_frame: bool,
) -> Result<Body, LoweringError> {
    let (buffer_captures, scalar_captures): (Vec<_>, Vec<_>) =
        captures.iter().partition(|c| !c.is_scalar);

    let frame_param_count = frame_uniform_param_count(uses_frame);
    let total_params = buffer_captures.len() + scalar_captures.len() + frame_param_count;
    let mut kernel = Body::new(total_params, span, ExecutionModel::GpuKernel);
    kernel
        .local_decls
        .push(LocalDecl::new(Type::new(TypeKind::Void, span), span));

    let block_size = BackendConfig::WEB_GPU.block_size(1);
    kernel.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some(block_size),
        grid_size: Some(range.grid),
        logical_extent: None,
        required_capabilities: Vec::new(),
        is_frame_step: true,
    }));

    let mut out_params: Vec<bool> = buffer_captures.iter().map(|c| c.is_written).collect();
    out_params.extend(std::iter::repeat_n(false, frame_param_count));
    out_params.extend(scalar_captures.iter().map(|_| false));
    kernel.out_params = out_params;
    let mut param_written: Vec<bool> = buffer_captures.iter().map(|c| c.writes_buffer).collect();
    param_written.resize(kernel.out_params.len(), false);
    kernel.param_written = param_written;

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

    forall_gpu::emit_1d_kernel_loop(
        &mut ctx,
        &range.axis,
        Some(range.grid),
        block_size,
        None,
        body,
        span,
    )?;
    Ok(ctx.body)
}

/// Register frame and runtime parameters for the kernel.
fn register_frame_runtime_params(
    ctx: &mut LoweringContext,
    buffer_captures: &[&forall_gpu::CaptureInfo],
    scalar_captures: &[&forall_gpu::CaptureInfo],
    uses_frame: bool,
    span: Span,
) -> crate::mir::Local {
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
            ctx.variable_map
                .insert(frame_input_param_key(idx).into(), field_local);
        }
    }

    let uniform_param =
        forall_gpu::push_launch_uniform(ctx, "_uniform_bound", LaunchUniform::LoopBound, span);

    for cap in scalar_captures {
        let local = ctx.push_param(cap.name.clone(), cap.ty.clone(), span);
        ctx.body.local_decls[local.0].storage_class = StorageClass::UniformBuffer;
    }

    uniform_param
}

fn build_frame_kernel_runtime(
    parent: &mut LoweringContext,
    captures: &[forall_gpu::CaptureInfo],
    axis: &forall_gpu::AxisSpec,
    body: &Statement,
    span: Span,
    uses_frame: bool,
) -> Result<Body, LoweringError> {
    let (buffer_captures, scalar_captures): (Vec<&_>, Vec<&_>) =
        captures.iter().partition(|c| !c.is_scalar);

    let frame_param_count = frame_uniform_param_count(uses_frame);
    let arg_count = captures.len() + 1 + frame_param_count;
    let mut kernel = Body::new(arg_count, span, ExecutionModel::GpuKernel);
    kernel
        .local_decls
        .push(LocalDecl::new(Type::new(TypeKind::Void, span), span));
    let block_size = BackendConfig::WEB_GPU.block_size(1);
    kernel.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some(block_size),
        grid_size: None,
        logical_extent: None,
        required_capabilities: Vec::new(),
        is_frame_step: true,
    }));

    let mut out_params: Vec<bool> = buffer_captures.iter().map(|c| c.is_written).collect();
    out_params.extend(std::iter::repeat_n(false, frame_param_count));
    out_params.push(false);
    out_params.extend(scalar_captures.iter().map(|_| false));
    kernel.out_params = out_params;
    let mut param_written: Vec<bool> = buffer_captures.iter().map(|c| c.writes_buffer).collect();
    param_written.resize(kernel.out_params.len(), false);
    kernel.param_written = param_written;

    let mut ctx = LoweringContext::new(kernel, parent.type_checker, parent.is_release);

    let uniform_param = register_frame_runtime_params(
        &mut ctx,
        &buffer_captures,
        &scalar_captures,
        uses_frame,
        span,
    );

    forall_gpu::emit_1d_kernel_loop(
        &mut ctx,
        axis,
        None,
        block_size,
        Some(uniform_param),
        body,
        span,
    )?;

    Ok(ctx.body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::node::IdNode;

    fn ident(name: &str) -> Expression {
        IdNode::new(
            0,
            ExpressionKind::Identifier(name.to_string(), None),
            Span::default(),
        )
    }

    /// `frame.time` member access — the atom `detect_frame_usage_expr` flags.
    fn frame_access() -> Expression {
        IdNode::new(
            0,
            ExpressionKind::Member(Box::new(ident("frame")), Box::new(ident("time"))),
            Span::default(),
        )
    }

    fn stmt(kind: StatementKind) -> Statement {
        Statement {
            id: 0,
            node: kind,
            span: Span::default(),
            trivia: Default::default(),
        }
    }

    #[test]
    fn detect_frame_usage_finds_frame_in_gpu_frame_block() {
        let inner = stmt(StatementKind::Expression(frame_access()));
        let block = stmt(StatementKind::Block(vec![inner]));
        let frame_block = stmt(StatementKind::GpuFrameBlock(Box::new(block)));
        assert!(detect_frame_usage(&frame_block));
    }

    #[test]
    fn detect_frame_usage_finds_frame_in_return() {
        let ret = stmt(StatementKind::Return(Some(Box::new(frame_access()))));
        assert!(detect_frame_usage(&ret));
    }

    #[test]
    fn detect_frame_usage_finds_frame_nested_in_cast() {
        // `(frame.time * 2.0) as f32` — the frame read is buried under a binary
        // and a cast. If the scan stops at the cast the frame inputs are never
        // injected, and the kernel body fails codegen ("Identifier as scalar").
        let mul = IdNode::new(
            1,
            ExpressionKind::Binary(
                Box::new(frame_access()),
                crate::ast::operator::BinaryOp::Mul,
                Box::new(IdNode::new(
                    2,
                    ExpressionKind::Literal(crate::ast::literal::Literal::Float(
                        crate::ast::literal::FloatLiteral::F64(2.0f64.to_bits()),
                    )),
                    Span::default(),
                )),
            ),
            Span::default(),
        );
        let cast = IdNode::new(
            3,
            ExpressionKind::Cast(Box::new(mul), Box::new(ident("f32"))),
            Span::default(),
        );
        assert!(detect_frame_usage(&stmt(StatementKind::Expression(cast))));
    }

    #[test]
    fn detect_frame_usage_false_for_frameless_statements() {
        assert!(!detect_frame_usage(&stmt(StatementKind::Return(None))));
        assert!(!detect_frame_usage(&stmt(StatementKind::Break)));
        assert!(!detect_frame_usage(&stmt(StatementKind::Empty)));
    }

    #[test]
    fn frame_uniform_param_count_tracks_field_list() {
        assert_eq!(frame_uniform_param_count(true), FRAME_INPUT_FIELDS.len());
        assert_eq!(frame_uniform_param_count(false), 0);
    }
}
