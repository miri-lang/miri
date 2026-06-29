// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR lowering for `forall` loops that target GPU accelerators.
//!
//! Extracts the loop body into a synthesized anonymous `gpu fn` kernel and
//! emits a `TerminatorKind::GpuLaunch` at the call site.
//!
//! Range bound modes:
//! - Range start must be an Int literal.
//! - Range end may be a runtime Int expression (e.g., `let n = 4; forall i in 0..n`).
//!   When end is a literal, uses fast constant-grid path.
//!   When end is a runtime expression, computes grid at runtime and passes the
//!   bounds-check limit as a uniform buffer to the kernel.
//!
//! Other restrictions:
//! - Accepts 1, 2, or 3 loop variables (1D, 2D, and 3D all supported).
//! - The body may reference outer-scope variables whose types are GPU
//!   buffers (`Array<T, N>`); all such captures are exposed as read-write
//!   storage buffers.

use std::collections::HashSet;

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::statement::{Statement, StatementKind, VariableDeclaration};
use crate::ast::types::{
    resolve_element_type_kind, BuiltinCollectionKind, Type, TypeKind, DIM3_TYPE_NAME,
};
use crate::ast::RangeExpressionType;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::backend::{BackendConfig, BackendMetadata, GpuBodyMetadata};
use crate::mir::body::{BindingResidency, DeviceHandleId};
use crate::mir::lambda::LambdaInfo;
use crate::mir::{
    AggregateKind, BinOp, Body, Constant, Dimension, Discriminant, ExecutionModel, GpuIntrinsic,
    GpuLaunchArgs, Local, LocalDecl, Operand, Place, Rvalue, Statement as MirStatement,
    StatementKind as MirStatementKind, StorageClass, Terminator, TerminatorKind,
};

use super::context::LoweringContext;
use super::expression::lower_expression;
use super::statement::lower_statement;

pub const FORALL_GPU_BLOCK_SIZE: u32 = 256;

/// Array of bounds (one per axis: X, Y, Z).
type BoundsArray = [Option<Box<Operand>>; 3];

/// Parameters for GPU launch terminator assembly.
struct GpuLaunchTerminatorParams<'a> {
    kernel_name: &'a str,
    captures: &'a [CaptureInfo],
    grid_local: Local,
    block_local: Local,
    bounds: BoundsArray,
}

/// Specifies the bound (end value) for a single axis in an N-D forall loop.
///
/// - `Literal(end, range_type)`: compile-time-constant end value (fast path, fixed grid size).
/// - `Runtime(end_expr, range_type)`: runtime-computed end value (grid computed at runtime,
///   bound passed via uniform buffer).
#[derive(Debug, Clone)]
pub enum AxisBound {
    Literal(i64, RangeExpressionType),
    Runtime(Expression, RangeExpressionType),
}

/// Specifies one axis (dimension) of an N-D forall loop.
///
/// Fields describe the iteration bounds and the loop variable name.
/// Used to unify 1D, 2D, and 3D lowering into a single N-D path.
#[derive(Debug, Clone)]
pub struct AxisSpec {
    /// The loop variable name (e.g., "i", "j", "k").
    pub name: String,
    /// Start value (must be an integer literal).
    pub start: i64,
    /// Loop dimension (X=0, Y=1, Z=2). Derived from axis index.
    pub dimension: Dimension,
    /// End bound (literal or runtime expression).
    pub bound: AxisBound,
}

/// Lowers a `forall` loop targeting GPU into a synthesized kernel + `GpuLaunch`.
///
/// Unified N-D entry point that handles 1D, 2D, and 3D loops.
///
/// # Arguments
///
/// - `config`: Backend configuration specifying block sizes for the target GPU.
pub fn lower_forall_gpu(
    ctx: &mut LoweringContext,
    config: &BackendConfig,
    span: &Span,
    stmt_id: usize,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) -> Result<(), LoweringError> {
    let rank = decls.len();
    if !(1..=3).contains(&rank) {
        return Err(LoweringError::unsupported_expression(
            format!("forall: expected 1, 2, or 3 loop variables, got {}", rank),
            *span,
        ));
    }

    let axes = extract_axes(decls, iterable, span, rank)?;
    let loop_var_name = decls[0].name.clone();
    let captures = collect_capture_infos(ctx, body, &loop_var_name, *span)?;

    let kernel_name = format!("miri_gpu_forall_{}", stmt_id);

    let kernel_body = build_kernel_body_nd(ctx, config, &axes, &captures, body, *span)?;

    ctx.lambda_bodies.push(LambdaInfo {
        name: kernel_name.clone(),
        body: kernel_body,
        captures: Vec::new(),
    });

    emit_gpu_launch_nd(ctx, config, &kernel_name, &axes, &captures, *span)?;

    Ok(())
}

/// Builds an AxisSpec from a range expression and variable declaration.
fn axis_from_range(
    var_decl: &VariableDeclaration,
    range_expr: &Expression,
    dimension: Dimension,
    span: Span,
) -> Result<AxisSpec, LoweringError> {
    let ExpressionKind::Range(start, Some(end), range_type) = &range_expr.node else {
        return Err(LoweringError::unsupported_expression(
            "forall range must be a bounded numeric range like '0..n'".to_string(),
            span,
        ));
    };
    let start_lit = read_int_literal(start, span)?;
    let bound = if matches!(&end.node, ExpressionKind::Literal(Literal::Integer(_))) {
        AxisBound::Literal(read_int_literal(end, span)?, range_type.clone())
    } else {
        AxisBound::Runtime((**end).clone(), range_type.clone())
    };
    Ok(AxisSpec {
        name: var_decl.name.clone(),
        start: start_lit,
        dimension,
        bound,
    })
}

/// Extracts axis specifications from the iterable and variable declarations.
fn extract_axes(
    decls: &[VariableDeclaration],
    iterable: &Expression,
    span: &Span,
    rank: usize,
) -> Result<Vec<AxisSpec>, LoweringError> {
    match rank {
        1 => {
            let axis = axis_from_range(&decls[0], iterable, Dimension::X, *span)?;
            Ok(vec![axis])
        }
        2 => {
            let ExpressionKind::Tuple(ranges) = &iterable.node else {
                return Err(LoweringError::unsupported_expression(
                    "2D forall: expected a tuple of two ranges".to_string(),
                    *span,
                ));
            };
            if ranges.len() != 2 {
                return Err(LoweringError::unsupported_expression(
                    format!("2D forall: expected exactly 2 ranges, got {}", ranges.len()),
                    *span,
                ));
            }
            let mut axes = Vec::new();
            for (idx, (range_expr, var_decl)) in ranges.iter().zip(&decls[0..2]).enumerate() {
                let dimension = if idx == 0 { Dimension::X } else { Dimension::Y };
                let axis = axis_from_range(var_decl, range_expr, dimension, *span)?;
                axes.push(axis);
            }
            Ok(axes)
        }
        3 => {
            let ExpressionKind::Tuple(ranges) = &iterable.node else {
                return Err(LoweringError::unsupported_expression(
                    "3D forall: expected a tuple of three ranges".to_string(),
                    *span,
                ));
            };
            if ranges.len() != 3 {
                return Err(LoweringError::unsupported_expression(
                    format!("3D forall: expected exactly 3 ranges, got {}", ranges.len()),
                    *span,
                ));
            }
            let mut axes = Vec::new();
            for (idx, (range_expr, var_decl)) in ranges.iter().zip(&decls[0..3]).enumerate() {
                let dimension = match idx {
                    0 => Dimension::X,
                    1 => Dimension::Y,
                    2 => Dimension::Z,
                    _ => unreachable!(),
                };
                let axis = axis_from_range(var_decl, range_expr, dimension, *span)?;
                axes.push(axis);
            }
            Ok(axes)
        }
        _ => unreachable!("rank validated to 1..=3 above"),
    }
}

/// Computes kernel grid size for literal mode.
fn compute_kernel_grid_size(
    axes: &[AxisSpec],
    block: [u32; 3],
    runtime: bool,
    span: Span,
) -> Result<Option<[u32; 3]>, LoweringError> {
    if runtime {
        return Ok(None);
    }

    let mut grid = [1u32; 3];
    for (i, axis) in axes.iter().enumerate() {
        if let AxisBound::Literal(end, range_type) = &axis.bound {
            let length = compute_range_length(axis.start, *end, range_type.clone(), span)?;
            grid[i] = literal_grid_dim(length, block[i]);
        } else {
            unreachable!("literal mode checked above");
        }
    }
    Ok(Some(grid))
}

/// Builds loop local variables from thread indices and axis starts.
fn build_loop_locals(ctx: &mut LoweringContext, axes: &[AxisSpec], span: Span) -> Vec<Local> {
    let i64_ty = Type::new(TypeKind::Int, span);
    let mut loop_locals = Vec::new();

    for axis in axes {
        let thread_int = compute_thread_index(ctx, axis.dimension, span);
        let loop_local = ctx.push_local(axis.name.clone(), i64_ty.clone(), span);
        push_assign(
            ctx,
            loop_local,
            Rvalue::BinaryOp(
                BinOp::Add,
                Box::new(Operand::Copy(Place::new(thread_int))),
                Box::new(int_constant(axis.start, span)),
            ),
            span,
        );
        loop_locals.push(loop_local);
    }

    loop_locals
}

/// Pushes kernel parameters: buffer globals, scalar uniforms, and optional bounds.
/// Returns the uniform bounds locals for runtime mode (empty for literal mode).
fn push_kernel_params(
    ctx: &mut LoweringContext,
    axes: &[AxisSpec],
    buffer_captures: &[&CaptureInfo],
    scalar_captures: &[&CaptureInfo],
    runtime: bool,
    span: Span,
) -> Vec<Local> {
    for cap in buffer_captures {
        let local = ctx.push_param(cap.name.clone(), cap.ty.clone(), span);
        ctx.body.local_decls[local.0].storage_class = StorageClass::GpuGlobal;
    }

    for cap in scalar_captures {
        let local = ctx.push_param(cap.name.clone(), cap.ty.clone(), span);
        ctx.body.local_decls[local.0].storage_class = StorageClass::UniformBuffer;
    }

    let mut uniform_bounds = Vec::new();
    if runtime {
        for axis in axes {
            let bound_name = match axis.dimension {
                Dimension::X => "_bound_x",
                Dimension::Y => "_bound_y",
                Dimension::Z => "_bound_z",
            };
            let local =
                ctx.push_param(bound_name.to_string(), Type::new(TypeKind::Int, span), span);
            ctx.body.local_decls[local.0].storage_class = StorageClass::UniformBuffer;
            uniform_bounds.push(local);
        }
    }
    uniform_bounds
}

/// Builds an N-D kernel body for a forall GPU loop.
/// Handles both literal and runtime bounds uniformly via slice-driven axes.
///
/// # Algorithm
/// - Rank determined by `axes.len()` (1..=3).
/// - Detect mode: `runtime = axes.iter().any(|a| matches!(a.bound, AxisBound::Runtime(_)))`
/// - Literal mode: compile-time grid, no uniform params for bounds.
/// - Runtime mode: runtime grid, each axis gets a `_bound_x/_bound_y/_bound_z` uniform param.
/// - Emit N-D bounds check: fold left-associative AND over `(loop_var_i < limit_i)` for each axis.
fn build_kernel_body_nd(
    parent: &mut LoweringContext,
    config: &BackendConfig,
    axes: &[AxisSpec],
    captures: &[CaptureInfo],
    body: &Statement,
    span: Span,
) -> Result<Body, LoweringError> {
    let rank = axes.len();
    let (buffer_captures, scalar_captures): (Vec<_>, Vec<_>) =
        captures.iter().partition(|c| !c.is_scalar);

    let runtime = axes
        .iter()
        .any(|a| matches!(a.bound, AxisBound::Runtime(..)));

    let bound_count = if runtime { rank } else { 0 };
    let arg_count = buffer_captures.len() + scalar_captures.len() + bound_count;

    let mut kernel = Body::new(arg_count, span, ExecutionModel::GpuKernel);
    kernel
        .local_decls
        .push(LocalDecl::new(Type::new(TypeKind::Void, span), span));

    let block = config.block_size(rank);
    let grid_size = compute_kernel_grid_size(axes, block, runtime, span)?;

    kernel.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some(block),
        grid_size,
        required_capabilities: Vec::new(),
        is_frame_step: false,
    }));

    let mut out_params: Vec<bool> = buffer_captures.iter().map(|c| c.is_written).collect();
    out_params.extend(scalar_captures.iter().map(|_| false));
    out_params.extend(std::iter::repeat_n(false, bound_count));
    kernel.out_params = out_params;

    let mut ctx = LoweringContext::new(kernel, parent.type_checker, parent.is_release);

    let uniform_bounds = push_kernel_params(
        &mut ctx,
        axes,
        &buffer_captures,
        &scalar_captures,
        runtime,
        span,
    );

    let loop_locals = build_loop_locals(&mut ctx, axes, span);

    emit_bounds_check_nd(
        &mut ctx,
        axes,
        &loop_locals,
        &uniform_bounds,
        runtime,
        body,
        span,
    )?;

    Ok(ctx.body)
}

/// Builds a per-axis bounds check condition: `loop_var_i < limit_i`.
fn build_per_axis_check(
    ctx: &mut LoweringContext,
    axis: &AxisSpec,
    loop_local: Local,
    uniform_bound: Option<Local>,
    span: Span,
) -> Result<Local, LoweringError> {
    let i64_ty = Type::new(TypeKind::Int, span);

    let limit_operand = if let Some(uniform_local) = uniform_bound {
        let uniform_cast_local = ctx.push_temp(i64_ty.clone(), span);
        push_assign(
            ctx,
            uniform_cast_local,
            Rvalue::Cast(
                Box::new(Operand::Copy(Place::new(uniform_local))),
                i64_ty.clone(),
            ),
            span,
        );
        Operand::Copy(Place::new(uniform_cast_local))
    } else {
        if let AxisBound::Literal(end, range_type) = &axis.bound {
            let limit = axis
                .start
                .checked_add(compute_range_length(
                    axis.start,
                    *end,
                    range_type.clone(),
                    span,
                )?)
                .ok_or_else(|| bounds_overflow_err(span))?;
            int_constant(limit, span)
        } else {
            unreachable!("runtime bounds should have uniform_bound");
        }
    };

    let check_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        ctx,
        check_local,
        Rvalue::BinaryOp(
            BinOp::Lt,
            Box::new(Operand::Copy(Place::new(loop_local))),
            Box::new(limit_operand),
        ),
        span,
    );

    Ok(check_local)
}

/// Folds per-axis conditions into a single AND condition.
fn fold_conditions(ctx: &mut LoweringContext, conditions: &[Local], span: Span) -> Local {
    if conditions.len() == 1 {
        return conditions[0];
    }

    let mut result = conditions[0];
    for &check in &conditions[1..] {
        let and_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
        push_assign(
            ctx,
            and_local,
            Rvalue::BinaryOp(
                BinOp::BitAnd,
                Box::new(Operand::Copy(Place::new(result))),
                Box::new(Operand::Copy(Place::new(check))),
            ),
            span,
        );
        result = and_local;
    }
    result
}

/// Emits an N-D bounds check for forall GPU loops.
/// Handles 1, 2, or 3 axes uniformly by folding a left-associative AND over
/// the per-axis condition `(loop_var_i < limit_i)`.
fn emit_bounds_check_nd(
    ctx: &mut LoweringContext,
    axes: &[AxisSpec],
    loop_locals: &[Local],
    uniform_bounds: &[Local],
    runtime: bool,
    body: &Statement,
    span: Span,
) -> Result<(), LoweringError> {
    let mut bound_checks = Vec::new();
    for (i, axis) in axes.iter().enumerate() {
        let loop_local = loop_locals[i];
        let uniform_bound = if runtime {
            Some(uniform_bounds[i])
        } else {
            None
        };
        let check = build_per_axis_check(ctx, axis, loop_local, uniform_bound, span)?;
        bound_checks.push(check);
    }

    let final_cond = fold_conditions(ctx, &bound_checks, span);

    let body_bb = ctx.new_basic_block();
    let exit_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(final_cond)),
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

/// Converts an operand to a local variable if it's not already one.
/// If operand is a simple Copy of an existing local, returns that local.
/// Otherwise materializes to a temp local.
fn operand_to_local(ctx: &mut LoweringContext, op: &Operand, span: Span) -> Local {
    if let Operand::Copy(Place { local, projection }) = op {
        if projection.is_empty() {
            return *local;
        }
    }
    let i64_ty = Type::new(TypeKind::Int, span);
    let temp = ctx.push_temp(i64_ty, span);
    push_assign(ctx, temp, Rvalue::Use(op.clone()), span);
    temp
}

/// Computes end operand for a single axis (literal or runtime).
fn compute_axis_end_op(
    ctx: &mut LoweringContext,
    axis: &AxisSpec,
    i64_ty: &Type,
    span: Span,
) -> Result<Operand, LoweringError> {
    match &axis.bound {
        AxisBound::Literal(end, range_type) => {
            let length = compute_range_length(axis.start, *end, range_type.clone(), span)?;
            let end_val = axis
                .start
                .checked_add(length)
                .ok_or_else(|| bounds_overflow_err(span))?;
            Ok(materialize_operand_to_local(
                ctx,
                int_constant(end_val, span),
                span,
            ))
        }
        AxisBound::Runtime(end_expr, range_type) => {
            let mut op = lower_expression(ctx, end_expr, None)?;
            if *range_type == RangeExpressionType::Inclusive {
                let plus_one_local = ctx.push_temp(i64_ty.clone(), span);
                push_assign(
                    ctx,
                    plus_one_local,
                    Rvalue::BinaryOp(BinOp::Add, Box::new(op), Box::new(int_constant(1, span))),
                    span,
                );
                op = Operand::Copy(Place::new(plus_one_local));
            }
            Ok(materialize_operand_to_local(ctx, op, span))
        }
    }
}

/// Computes runtime grid dimensions and bounds for forall GPU loop.
/// Returns (grid_local, [bound_x, bound_y, bound_z]).
fn compute_runtime_grid_and_bounds(
    ctx: &mut LoweringContext,
    axes: &[AxisSpec],
    block_size: &[u32; 3],
    span: Span,
) -> Result<(Local, BoundsArray), LoweringError> {
    let dim3_ty = Type::new(TypeKind::Custom(DIM3_TYPE_NAME.to_string(), None), span);
    let i64_ty = Type::new(TypeKind::Int, span);

    let mut grid_values = vec![int_constant(1, span); 3];
    let mut bounds: [Option<Box<Operand>>; 3] = [None, None, None];

    for (i, axis) in axes.iter().enumerate() {
        let end_op = compute_axis_end_op(ctx, axis, &i64_ty, span)?;
        let clamped_length = compute_clamped_length(ctx, end_op.clone(), axis.start, span);
        let grid_dim = compute_grid_size(ctx, clamped_length, block_size[i], span);
        grid_values[i] = Operand::Copy(Place::new(grid_dim));

        bounds[i] = Some(Box::new(end_op));
    }

    let grid_x = operand_to_local(ctx, &grid_values[0], span);
    let grid_y = operand_to_local(ctx, &grid_values[1], span);
    let grid_z = operand_to_local(ctx, &grid_values[2], span);

    let grid_local = ctx.push_temp(dim3_ty.clone(), span);
    push_assign(
        ctx,
        grid_local,
        Rvalue::Aggregate(
            AggregateKind::Struct(dim3_ty),
            vec![
                Operand::Copy(Place::new(grid_x)),
                Operand::Copy(Place::new(grid_y)),
                Operand::Copy(Place::new(grid_z)),
            ],
        ),
        span,
    );

    Ok((grid_local, bounds))
}

/// Builds literal (compile-time) grid Dim3 for forall GPU loop.
fn build_literal_grid_dim3(
    ctx: &mut LoweringContext,
    axes: &[AxisSpec],
    block_size: &[u32; 3],
    span: Span,
) -> Result<Local, LoweringError> {
    let dim3_ty = Type::new(TypeKind::Custom(DIM3_TYPE_NAME.to_string(), None), span);
    let mut grid = [1u32; 3];

    for (i, axis) in axes.iter().enumerate() {
        if let AxisBound::Literal(end, range_type) = &axis.bound {
            let length = compute_range_length(axis.start, *end, range_type.clone(), span)?;
            grid[i] = literal_grid_dim(length, block_size[i]);
        }
    }

    let grid_local = ctx.push_temp(dim3_ty.clone(), span);
    push_assign(
        ctx,
        grid_local,
        Rvalue::Aggregate(
            AggregateKind::Struct(dim3_ty),
            vec![
                int_constant(i64::from(grid[0]), span),
                int_constant(i64::from(grid[1]), span),
                int_constant(i64::from(grid[2]), span),
            ],
        ),
        span,
    );

    Ok(grid_local)
}

/// Builds block size Dim3 for GPU kernel launch.
fn build_block_dim3(ctx: &mut LoweringContext, block_size: &[u32; 3], span: Span) -> Local {
    let dim3_ty = Type::new(TypeKind::Custom(DIM3_TYPE_NAME.to_string(), None), span);
    let block_local = ctx.push_temp(dim3_ty.clone(), span);
    push_assign(
        ctx,
        block_local,
        Rvalue::Aggregate(
            AggregateKind::Struct(dim3_ty),
            vec![
                int_constant(i64::from(block_size[0]), span),
                int_constant(i64::from(block_size[1]), span),
                int_constant(i64::from(block_size[2]), span),
            ],
        ),
        span,
    );
    block_local
}

/// Assembles GpuLaunch terminator with grid, block, arguments, and bounds.
fn assemble_gpu_launch_terminator(
    ctx: &mut LoweringContext,
    params: GpuLaunchTerminatorParams,
    span: Span,
) -> Result<(), LoweringError> {
    let (buffer_captures, scalar_captures): (Vec<_>, Vec<_>) =
        params.captures.iter().partition(|c| !c.is_scalar);

    let kernel_op = Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: Literal::Identifier(params.kernel_name.to_string()),
    }));

    let buffer_ops: Vec<Operand> = buffer_captures
        .iter()
        .map(|c| Operand::Copy(Place::new(c.outer_local)))
        .collect();
    let scalar_ops: Vec<Operand> = scalar_captures
        .iter()
        .map(|c| Operand::Copy(Place::new(c.outer_local)))
        .collect();

    let arg_handles: Vec<Option<DeviceHandleId>> = buffer_captures
        .iter()
        .map(|c| ctx.body.local_decls[c.outer_local.0].device_handle)
        .collect();
    let arg_read_only: Vec<bool> = buffer_captures.iter().map(|c| !c.is_written).collect();
    let arg_int_narrow: Vec<bool> = buffer_captures
        .iter()
        .map(|c| needs_int_narrowing(&c.ty))
        .collect();
    let launch_args = GpuLaunchArgs::new(buffer_ops, arg_handles, arg_read_only, arg_int_narrow)
        .map_err(|e| LoweringError::custom(e.to_string(), span, None))?;

    let void_ty = Type::new(TypeKind::Void, span);
    let dest_local = ctx.push_temp(void_ty, span);
    let after_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::GpuLaunch {
            kernel: kernel_op,
            grid: Operand::Copy(Place::new(params.grid_local)),
            block: Operand::Copy(Place::new(params.block_local)),
            launch_args,
            scalar_args: scalar_ops,
            uniform_bound_x: params.bounds[0].clone(),
            uniform_bound_y: params.bounds[1].clone(),
            uniform_bound_z: params.bounds[2].clone(),
            destination: Place::new(dest_local),
            target: Some(after_bb),
        },
        span,
    ));
    ctx.set_current_block(after_bb);
    Ok(())
}

/// Emits a GpuLaunch terminator for an N-D forall GPU loop.
/// Detects literal vs runtime mode and emits accordingly.
fn emit_gpu_launch_nd(
    ctx: &mut LoweringContext,
    config: &BackendConfig,
    kernel_name: &str,
    axes: &[AxisSpec],
    captures: &[CaptureInfo],
    span: Span,
) -> Result<(), LoweringError> {
    let rank = axes.len();
    let block = config.block_size(rank);
    let block_size = [block[0], block[1], block[2]];

    let runtime = axes
        .iter()
        .any(|a| matches!(a.bound, AxisBound::Runtime(..)));

    let (grid_local, bounds) = if runtime {
        compute_runtime_grid_and_bounds(ctx, axes, &block_size, span)?
    } else {
        let grid_local = build_literal_grid_dim3(ctx, axes, &block_size, span)?;
        (grid_local, [None, None, None])
    };

    let block_local = build_block_dim3(ctx, &block_size, span);

    assemble_gpu_launch_terminator(
        ctx,
        GpuLaunchTerminatorParams {
            kernel_name,
            captures,
            grid_local,
            block_local,
            bounds,
        },
        span,
    )
}

fn collect_written_captures(body: &Statement) -> HashSet<String> {
    let mut written = HashSet::new();
    visit_written_stmt(body, &mut written);
    written
}

fn visit_written_stmt(stmt: &Statement, written: &mut HashSet<String>) {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            for s in stmts {
                visit_written_stmt(s, written);
            }
        }
        StatementKind::Expression(expr) => visit_written_expr(expr, written),
        StatementKind::Variable(_, _) => {}
        StatementKind::Return(_) => {}
        StatementKind::If(_, then_branch, else_branch, _) => {
            visit_written_stmt(then_branch, written);
            if let Some(eb) = else_branch {
                visit_written_stmt(eb, written);
            }
        }
        StatementKind::While(_, body, _) => visit_written_stmt(body, written),
        StatementKind::For(_, _, body) | StatementKind::GpuFrame(_, _, body) => {
            visit_written_stmt(body, written);
        }
        StatementKind::Forall { body, .. } => {
            visit_written_stmt(body, written);
        }
        StatementKind::GpuFrameBlock(block) => {
            visit_written_stmt(block, written);
        }
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Use(_, _)
        | StatementKind::Type(_, _)
        | StatementKind::FunctionDeclaration(_)
        | StatementKind::Enum(_, _, _, _, _, _)
        | StatementKind::Struct(_, _, _, _, _)
        | StatementKind::Class(_)
        | StatementKind::Trait(_, _, _, _, _)
        | StatementKind::RuntimeFunctionDeclaration(_, _, _, _)
        | StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => {}
    }
}

fn visit_written_expr(expr: &Expression, written: &mut HashSet<String>) {
    match &expr.node {
        ExpressionKind::Assignment(lhs, _, rhs) => {
            extract_written_lhs(lhs, written);
            visit_written_expr(rhs, written);
        }
        ExpressionKind::Call(func, args) => {
            if let ExpressionKind::Identifier(name, _) = &func.node {
                if crate::mir::backend::gpu::GpuAtomicOp::from_builtin_name(name).is_some() {
                    if let Some(buf) = args.first() {
                        if let ExpressionKind::Identifier(buf_name, _) = &buf.node {
                            written.insert(buf_name.clone());
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn extract_written_lhs(
    lhs: &crate::ast::expression::LeftHandSideExpression,
    written: &mut HashSet<String>,
) {
    use crate::ast::expression::LeftHandSideExpression;
    match lhs {
        LeftHandSideExpression::Identifier(expr) => {
            if let ExpressionKind::Identifier(name, _) = &expr.node {
                written.insert(name.clone());
            }
        }
        LeftHandSideExpression::Index(expr) | LeftHandSideExpression::Member(expr) => {
            if let ExpressionKind::Index(base, _) | ExpressionKind::Member(base, _) = &expr.node {
                if let ExpressionKind::Identifier(name, _) = &base.node {
                    written.insert(name.clone());
                }
            }
        }
    }
}

fn visit_lhs(
    lhs: &crate::ast::expression::LeftHandSideExpression,
    bound: &mut HashSet<String>,
    ctx: &LoweringContext,
    seen: &mut HashSet<String>,
    ordered: &mut Vec<String>,
) {
    use crate::ast::expression::LeftHandSideExpression;
    match lhs {
        LeftHandSideExpression::Identifier(expr)
        | LeftHandSideExpression::Member(expr)
        | LeftHandSideExpression::Index(expr) => visit_expr(expr, bound, ctx, seen, ordered),
    }
}

fn buffer_has_atomic_element(kind: &TypeKind) -> bool {
    let elem_node = match kind {
        TypeKind::Custom(name, Some(args))
            if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Array)
                && !args.is_empty() =>
        {
            &args[0].node
        }
        TypeKind::Array(elem_expr, _) => &elem_expr.node,
        TypeKind::List(elem_expr) => &elem_expr.node,
        _ => return false,
    };
    matches!(
        elem_node,
        crate::ast::expression::ExpressionKind::Type(elem_ty, _)
            if matches!(&elem_ty.kind, TypeKind::Custom(n, _) if n == crate::ast::types::ATOMIC_TYPE_NAME)
    )
}

fn is_gpu_buffer_capture(kind: &TypeKind) -> bool {
    match kind {
        TypeKind::Array(..) | TypeKind::List(..) => true,
        TypeKind::Custom(name, Some(args)) => {
            matches!(
                BuiltinCollectionKind::from_name(name),
                Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
            ) && !args.is_empty()
        }
        TypeKind::Custom(name, None) => matches!(
            BuiltinCollectionKind::from_name(name),
            Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
        ),
        _ => false,
    }
}

fn is_gpu_scalar_capture(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::Int
            | TypeKind::I8
            | TypeKind::I16
            | TypeKind::I32
            | TypeKind::I64
            | TypeKind::I128
            | TypeKind::U8
            | TypeKind::U16
            | TypeKind::U32
            | TypeKind::U64
            | TypeKind::U128
            | TypeKind::Float
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::Boolean
    )
}

pub fn int_literal_to_i64(lit: &IntegerLiteral) -> i64 {
    match lit {
        IntegerLiteral::I8(v) => i64::from(*v),
        IntegerLiteral::I16(v) => i64::from(*v),
        IntegerLiteral::I32(v) => i64::from(*v),
        IntegerLiteral::I64(v) => *v,
        IntegerLiteral::I128(v) => (*v).try_into().unwrap_or(i64::MAX),
        IntegerLiteral::U8(v) => i64::from(*v),
        IntegerLiteral::U16(v) => i64::from(*v),
        IntegerLiteral::U32(v) => i64::from(*v),
        IntegerLiteral::U64(v) => i64::try_from(*v).unwrap_or(i64::MAX),
        IntegerLiteral::U128(v) => (*v).try_into().unwrap_or(i64::MAX),
    }
}

pub fn collect_outer_captures(
    body: &Statement,
    loop_var: &str,
    ctx: &LoweringContext,
) -> Vec<String> {
    let mut bound: HashSet<String> = HashSet::new();
    bound.insert(loop_var.to_string());

    let mut seen: HashSet<String> = HashSet::new();
    let mut ordered: Vec<String> = Vec::new();

    visit_stmt(body, &mut bound, ctx, &mut seen, &mut ordered);
    ordered
}

fn visit_stmt(
    stmt: &Statement,
    bound: &mut HashSet<String>,
    ctx: &LoweringContext,
    seen: &mut HashSet<String>,
    ordered: &mut Vec<String>,
) {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            let scope_snapshot = bound.clone();
            for s in stmts {
                visit_stmt(s, bound, ctx, seen, ordered);
            }
            *bound = scope_snapshot;
        }
        StatementKind::Expression(expr) => visit_expr(expr, bound, ctx, seen, ordered),
        StatementKind::Variable(decls, _) => {
            for d in decls {
                if let Some(init) = &d.initializer {
                    visit_expr(init, bound, ctx, seen, ordered);
                }
                bound.insert(d.name.clone());
            }
        }
        StatementKind::Return(Some(e)) => visit_expr(e, bound, ctx, seen, ordered),
        StatementKind::Return(None) => {}
        StatementKind::If(cond, then_branch, else_branch, _) => {
            visit_expr(cond, bound, ctx, seen, ordered);
            visit_stmt(then_branch, bound, ctx, seen, ordered);
            if let Some(eb) = else_branch {
                visit_stmt(eb, bound, ctx, seen, ordered);
            }
        }
        StatementKind::While(cond, body, _) => {
            visit_expr(cond, bound, ctx, seen, ordered);
            visit_stmt(body, bound, ctx, seen, ordered);
        }
        StatementKind::For(inner_decls, iter, body)
        | StatementKind::GpuFrame(inner_decls, iter, body) => {
            visit_expr(iter, bound, ctx, seen, ordered);
            let scope_snapshot = bound.clone();
            for d in inner_decls {
                bound.insert(d.name.clone());
            }
            visit_stmt(body, bound, ctx, seen, ordered);
            *bound = scope_snapshot;
        }
        StatementKind::Forall {
            vars,
            iterable,
            body,
            ..
        } => {
            visit_expr(iterable, bound, ctx, seen, ordered);
            let scope_snapshot = bound.clone();
            for d in vars {
                bound.insert(d.name.clone());
            }
            visit_stmt(body, bound, ctx, seen, ordered);
            *bound = scope_snapshot;
        }
        StatementKind::GpuFrameBlock(block) => {
            visit_stmt(block, bound, ctx, seen, ordered);
        }
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Use(_, _)
        | StatementKind::Type(_, _)
        | StatementKind::FunctionDeclaration(_)
        | StatementKind::Enum(_, _, _, _, _, _)
        | StatementKind::Struct(_, _, _, _, _)
        | StatementKind::Class(_)
        | StatementKind::Trait(_, _, _, _, _)
        | StatementKind::RuntimeFunctionDeclaration(_, _, _, _)
        | StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => {}
    }
}

fn visit_expr(
    expr: &Expression,
    bound: &mut HashSet<String>,
    ctx: &LoweringContext,
    seen: &mut HashSet<String>,
    ordered: &mut Vec<String>,
) {
    match &expr.node {
        ExpressionKind::Identifier(name, _) => {
            if !bound.contains(name)
                && ctx.variable_map.contains_key(name.as_str())
                && !seen.contains(name)
            {
                let local_idx = ctx.variable_map[name.as_str()];
                let ty = &ctx.body.local_decls[local_idx.0].ty;
                if !matches!(ty.kind, TypeKind::Function(_)) {
                    seen.insert(name.clone());
                    ordered.push(name.clone());
                }
            }
        }
        ExpressionKind::Binary(lhs, _, rhs) | ExpressionKind::Logical(lhs, _, rhs) => {
            visit_expr(lhs, bound, ctx, seen, ordered);
            visit_expr(rhs, bound, ctx, seen, ordered);
        }
        ExpressionKind::Unary(_, inner) | ExpressionKind::Guard(_, inner) => {
            visit_expr(inner, bound, ctx, seen, ordered)
        }
        ExpressionKind::Call(callee, args) => {
            visit_expr(callee, bound, ctx, seen, ordered);
            for a in args {
                visit_expr(a, bound, ctx, seen, ordered);
            }
        }
        ExpressionKind::Index(obj, idx) => {
            visit_expr(obj, bound, ctx, seen, ordered);
            visit_expr(idx, bound, ctx, seen, ordered);
        }
        ExpressionKind::Member(obj, prop) => {
            visit_expr(obj, bound, ctx, seen, ordered);
            visit_expr(prop, bound, ctx, seen, ordered);
        }
        ExpressionKind::Assignment(lhs, _, rhs) => {
            visit_lhs(lhs, bound, ctx, seen, ordered);
            visit_expr(rhs, bound, ctx, seen, ordered);
        }
        ExpressionKind::Conditional(cond, then_e, else_opt, _) => {
            visit_expr(cond, bound, ctx, seen, ordered);
            visit_expr(then_e, bound, ctx, seen, ordered);
            if let Some(else_e) = else_opt {
                visit_expr(else_e, bound, ctx, seen, ordered);
            }
        }
        ExpressionKind::Range(start, end, _) => {
            visit_expr(start, bound, ctx, seen, ordered);
            if let Some(end) = end {
                visit_expr(end, bound, ctx, seen, ordered);
            }
        }
        ExpressionKind::Array(elems, _)
        | ExpressionKind::List(elems)
        | ExpressionKind::Tuple(elems)
        | ExpressionKind::Set(elems)
        | ExpressionKind::FormattedString(elems) => {
            for e in elems {
                visit_expr(e, bound, ctx, seen, ordered);
            }
        }
        ExpressionKind::Map(entries) => {
            for (k, v) in entries {
                visit_expr(k, bound, ctx, seen, ordered);
                visit_expr(v, bound, ctx, seen, ordered);
            }
        }
        ExpressionKind::Match(scrutinee, branches) => {
            visit_expr(scrutinee, bound, ctx, seen, ordered);
            for b in branches {
                if let Some(guard) = &b.guard {
                    visit_expr(guard, bound, ctx, seen, ordered);
                }
                visit_stmt(&b.body, bound, ctx, seen, ordered);
            }
        }
        ExpressionKind::EnumValue(name_expr, args) => {
            visit_expr(name_expr, bound, ctx, seen, ordered);
            for a in args {
                visit_expr(a, bound, ctx, seen, ordered);
            }
        }
        ExpressionKind::NamedArgument(_, inner) => visit_expr(inner, bound, ctx, seen, ordered),
        ExpressionKind::Block(stmts, final_expr) => {
            let snap = bound.clone();
            for s in stmts {
                visit_stmt(s, bound, ctx, seen, ordered);
            }
            visit_expr(final_expr, bound, ctx, seen, ordered);
            *bound = snap;
        }
        ExpressionKind::Cast(value_expr, target_type_expr) => {
            visit_expr(value_expr, bound, ctx, seen, ordered);
            visit_expr(target_type_expr, bound, ctx, seen, ordered);
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::Super
        | ExpressionKind::Type(_, _)
        | ExpressionKind::GenericType(_, _, _)
        | ExpressionKind::TypeDeclaration(_, _, _, _)
        | ExpressionKind::ImportPath(_, _)
        | ExpressionKind::StructMember(_, _)
        | ExpressionKind::Lambda(_) => {}
    }
}

#[derive(Debug, Clone)]
pub struct CaptureInfo {
    pub name: String,
    pub ty: Type,
    pub outer_local: Local,
    pub is_scalar: bool,
    pub is_written: bool,
}

pub fn collect_capture_infos(
    ctx: &LoweringContext,
    body: &Statement,
    loop_var_name: &str,
    span: Span,
) -> Result<Vec<CaptureInfo>, LoweringError> {
    let capture_names = collect_outer_captures(body, loop_var_name, ctx);
    let written = collect_written_captures(body);
    let mut captures: Vec<CaptureInfo> = Vec::with_capacity(capture_names.len());

    for name in capture_names {
        let Some(&outer_local) = ctx.variable_map.get(name.as_str()) else {
            return Err(LoweringError::unsupported_expression(
                format!(
                    "forall: captured variable '{}' is not visible at the loop site",
                    name
                ),
                span,
            ));
        };
        let ty = ctx.body.local_decls[outer_local.0].ty.clone();

        let is_buffer = is_gpu_buffer_capture(&ty.kind);
        let is_scalar = is_gpu_scalar_capture(&ty.kind);

        if is_buffer {
            if ctx.body.local_decls[outer_local.0].residency != BindingResidency::Gpu {
                return Err(LoweringError::unsupported_expression(
                    format!("forall: capture '{}' is not gpu-resident", name),
                    span,
                ));
            }

            // Atomic-element buffers must bind `read_write`: WGSL requires
            // `atomic<u32>` storage to be read_write even when a pass only
            // atomicLoads it, so they are never treated as read-only.
            let is_written = written.contains(&name) || buffer_has_atomic_element(&ty.kind);
            captures.push(CaptureInfo {
                name,
                ty,
                outer_local,
                is_scalar: false,
                is_written,
            });
        } else if is_scalar {
            captures.push(CaptureInfo {
                name,
                ty,
                outer_local,
                is_scalar: true,
                is_written: false,
            });
        }
    }

    Ok(captures)
}

pub fn read_int_literal(expr: &Expression, span: Span) -> Result<i64, LoweringError> {
    match &expr.node {
        ExpressionKind::Literal(Literal::Integer(lit)) => Ok(int_literal_to_i64(lit)),
        _ => Err(LoweringError::unsupported_expression(
            "forall range bounds must be integer literals or simple variables".to_string(),
            span,
        )),
    }
}

pub fn bounds_overflow_err(span: Span) -> LoweringError {
    LoweringError::unsupported_expression(
        "forall range bounds would overflow i64".to_string(),
        span,
    )
}

pub fn compute_range_length(
    start: i64,
    end: i64,
    range_type: RangeExpressionType,
    span: Span,
) -> Result<i64, LoweringError> {
    let raw = match range_type {
        RangeExpressionType::Exclusive => end.checked_sub(start),
        RangeExpressionType::Inclusive => end.checked_sub(start).and_then(|d| d.checked_add(1)),
        RangeExpressionType::IterableObject => {
            return Err(LoweringError::unsupported_expression(
                "forall: iterable-object ranges are not supported (use 'a..b')".to_string(),
                span,
            ));
        }
    };
    let raw = raw.ok_or_else(|| bounds_overflow_err(span))?;
    if raw <= 0 {
        return Err(LoweringError::unsupported_expression(
            "forall: range length must be positive".to_string(),
            span,
        ));
    }
    Ok(raw)
}

pub fn compute_thread_index(ctx: &mut LoweringContext, dim: Dimension, span: Span) -> Local {
    let u32_ty = Type::new(TypeKind::U32, span);
    let i64_ty = Type::new(TypeKind::Int, span);

    let global_idx_u32 = ctx.push_temp(u32_ty.clone(), span);
    push_assign(
        ctx,
        global_idx_u32,
        Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(dim)),
        span,
    );
    let block_id_u32 = ctx.push_temp(u32_ty.clone(), span);
    push_assign(
        ctx,
        block_id_u32,
        Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(dim)),
        span,
    );
    let block_dim_u32 = ctx.push_temp(u32_ty.clone(), span);
    push_assign(
        ctx,
        block_dim_u32,
        Rvalue::GpuIntrinsic(GpuIntrinsic::BlockDim(dim)),
        span,
    );

    let grid_term_u32 = ctx.push_temp(u32_ty.clone(), span);
    push_assign(
        ctx,
        grid_term_u32,
        Rvalue::BinaryOp(
            BinOp::Mul,
            Box::new(Operand::Copy(Place::new(block_id_u32))),
            Box::new(Operand::Copy(Place::new(block_dim_u32))),
        ),
        span,
    );

    let global_idx_i64 = ctx.push_temp(i64_ty.clone(), span);
    push_assign(
        ctx,
        global_idx_i64,
        Rvalue::Cast(
            Box::new(Operand::Copy(Place::new(global_idx_u32))),
            i64_ty.clone(),
        ),
        span,
    );

    let grid_term_i64 = ctx.push_temp(i64_ty.clone(), span);
    push_assign(
        ctx,
        grid_term_i64,
        Rvalue::Cast(
            Box::new(Operand::Copy(Place::new(grid_term_u32))),
            i64_ty.clone(),
        ),
        span,
    );

    let global_idx = ctx.push_temp(i64_ty, span);
    push_assign(
        ctx,
        global_idx,
        Rvalue::BinaryOp(
            BinOp::Add,
            Box::new(Operand::Copy(Place::new(global_idx_i64))),
            Box::new(Operand::Copy(Place::new(grid_term_i64))),
        ),
        span,
    );

    global_idx
}

pub fn materialize_operand_to_local(ctx: &mut LoweringContext, op: Operand, span: Span) -> Operand {
    match &op {
        Operand::Copy(Place { local, projection }) | Operand::Move(Place { local, projection })
            if projection.is_empty() =>
        {
            op
        }
        _ => {
            let i64_ty = Type::new(TypeKind::Int, span);
            let local = ctx.push_temp(i64_ty, span);
            push_assign(ctx, local, Rvalue::Use(op), span);
            Operand::Copy(Place::new(local))
        }
    }
}

pub fn push_assign(ctx: &mut LoweringContext, local: Local, rvalue: Rvalue, span: Span) {
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::Assign(Place::new(local), rvalue),
        span,
    });
}

pub fn literal_grid_dim(extent: i64, block_size: u32) -> u32 {
    let block = i64::from(block_size);
    let count = extent / block + i64::from(extent % block != 0);
    count.min(u32::MAX as i64) as u32
}

pub fn literal_grid_x(length: i64, block_size: u32) -> u32 {
    literal_grid_dim(length, block_size)
}

pub fn int_constant(value: i64, span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Int, span),
        literal: Literal::Integer(IntegerLiteral::I64(value)),
    }))
}

pub fn needs_int_narrowing(ty: &Type) -> bool {
    let elem_expr = match &ty.kind {
        TypeKind::Array(elem_expr, _) | TypeKind::List(elem_expr) => Some(elem_expr.as_ref()),
        TypeKind::Custom(name, Some(args))
            if matches!(
                BuiltinCollectionKind::from_name(name),
                Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
            ) && !args.is_empty() =>
        {
            Some(&args[0])
        }
        _ => None,
    };

    elem_expr.is_some_and(|expr| {
        matches!(
            resolve_element_type_kind(expr),
            Some(TypeKind::Int | TypeKind::I64)
        )
    })
}

pub fn compute_grid_size(
    ctx: &mut LoweringContext,
    clamped_length: Local,
    block_size: u32,
    span: Span,
) -> Local {
    let i64_ty = Type::new(TypeKind::Int, span);
    let block_size_i64 = i64::from(block_size);

    let grid_div_local = ctx.push_temp(i64_ty.clone(), span);
    push_assign(
        ctx,
        grid_div_local,
        Rvalue::BinaryOp(
            BinOp::Div,
            Box::new(Operand::Copy(Place::new(clamped_length))),
            Box::new(int_constant(block_size_i64, span)),
        ),
        span,
    );
    let grid_rem_local = ctx.push_temp(i64_ty.clone(), span);
    push_assign(
        ctx,
        grid_rem_local,
        Rvalue::BinaryOp(
            BinOp::Rem,
            Box::new(Operand::Copy(Place::new(clamped_length))),
            Box::new(int_constant(block_size_i64, span)),
        ),
        span,
    );
    let has_remainder_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        ctx,
        has_remainder_local,
        Rvalue::BinaryOp(
            BinOp::Ne,
            Box::new(Operand::Copy(Place::new(grid_rem_local))),
            Box::new(int_constant(0, span)),
        ),
        span,
    );
    let has_remainder_i64_local = ctx.push_temp(i64_ty.clone(), span);
    push_assign(
        ctx,
        has_remainder_i64_local,
        Rvalue::Cast(
            Box::new(Operand::Copy(Place::new(has_remainder_local))),
            i64_ty.clone(),
        ),
        span,
    );
    let final_grid_local = ctx.push_temp(i64_ty, span);
    push_assign(
        ctx,
        final_grid_local,
        Rvalue::BinaryOp(
            BinOp::Add,
            Box::new(Operand::Copy(Place::new(grid_div_local))),
            Box::new(Operand::Copy(Place::new(has_remainder_i64_local))),
        ),
        span,
    );
    final_grid_local
}

pub fn compute_clamped_length(
    ctx: &mut LoweringContext,
    end_op: Operand,
    start: i64,
    span: Span,
) -> Local {
    let i64_ty = Type::new(TypeKind::Int, span);

    let length_local = ctx.push_temp(i64_ty.clone(), span);
    push_assign(
        ctx,
        length_local,
        Rvalue::BinaryOp(
            BinOp::Sub,
            Box::new(end_op.clone()),
            Box::new(int_constant(start, span)),
        ),
        span,
    );

    let is_in_range_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        ctx,
        is_in_range_local,
        Rvalue::BinaryOp(
            BinOp::Gt,
            Box::new(end_op),
            Box::new(int_constant(start, span)),
        ),
        span,
    );

    let is_in_range_i64_local = ctx.push_temp(i64_ty.clone(), span);
    push_assign(
        ctx,
        is_in_range_i64_local,
        Rvalue::Cast(
            Box::new(Operand::Copy(Place::new(is_in_range_local))),
            i64_ty.clone(),
        ),
        span,
    );

    let clamped_local = ctx.push_temp(i64_ty, span);
    push_assign(
        ctx,
        clamped_local,
        Rvalue::BinaryOp(
            BinOp::Mul,
            Box::new(Operand::Copy(Place::new(length_local))),
            Box::new(Operand::Copy(Place::new(is_in_range_i64_local))),
        ),
        span,
    );
    clamped_local
}

/// Emits bounds check loop with uniform parameter (for 1D GPU frame or forall).
/// Assumes loop_local and uniform_param are already initialized.
pub fn emit_bounds_check_loop(
    ctx: &mut LoweringContext,
    loop_local: Local,
    uniform_param: Local,
    body: &Statement,
    span: Span,
) -> Result<(), LoweringError> {
    let i64_ty = Type::new(TypeKind::Int, span);
    let uniform_cast_local = ctx.push_temp(i64_ty.clone(), span);
    push_assign(
        ctx,
        uniform_cast_local,
        Rvalue::Cast(Box::new(Operand::Copy(Place::new(uniform_param))), i64_ty),
        span,
    );

    let cond_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        ctx,
        cond_local,
        Rvalue::BinaryOp(
            BinOp::Lt,
            Box::new(Operand::Copy(Place::new(loop_local))),
            Box::new(Operand::Copy(Place::new(uniform_cast_local))),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_grid_x_small_range() {
        assert_eq!(literal_grid_x(256, 256), 1);
    }

    #[test]
    fn test_literal_grid_x_with_remainder() {
        assert_eq!(literal_grid_x(257, 256), 2);
    }

    #[test]
    fn test_literal_grid_x_zero() {
        assert_eq!(literal_grid_x(0, 256), 0);
    }

    #[test]
    fn test_literal_grid_x_large_clamped() {
        let length = 1_099_511_627_776_i64;
        assert_eq!(literal_grid_x(length, 256), u32::MAX);
    }

    #[test]
    fn test_literal_grid_x_i64_max() {
        assert_eq!(literal_grid_x(i64::MAX, 256), u32::MAX);
    }

    #[test]
    fn test_literal_grid_dim_2d_small_range() {
        const BLOCK_SIZE_2D: u32 = 16;
        assert_eq!(literal_grid_dim(16, BLOCK_SIZE_2D), 1);
    }

    #[test]
    fn test_literal_grid_dim_2d_with_remainder() {
        const BLOCK_SIZE_2D: u32 = 16;
        assert_eq!(literal_grid_dim(17, BLOCK_SIZE_2D), 2);
    }

    #[test]
    fn test_literal_grid_dim_2d_zero() {
        const BLOCK_SIZE_2D: u32 = 16;
        assert_eq!(literal_grid_dim(0, BLOCK_SIZE_2D), 0);
    }

    #[test]
    fn test_literal_grid_dim_2d_large_clamped() {
        const BLOCK_SIZE_2D: u32 = 16;
        let extent = 68_719_476_736_i64;
        assert_eq!(literal_grid_dim(extent, BLOCK_SIZE_2D), u32::MAX);
    }

    #[test]
    fn test_literal_grid_dim_2d_i64_max() {
        const BLOCK_SIZE_2D: u32 = 16;
        assert_eq!(literal_grid_dim(i64::MAX, BLOCK_SIZE_2D), u32::MAX);
    }
}
