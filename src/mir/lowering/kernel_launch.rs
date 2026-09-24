// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU kernel launch lowering.

use crate::ast::expression::Expression;
use crate::ast::gpu_wire::scalar_capture_wire;
use crate::ast::{ExpressionKind, Type, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{GpuLaunchArgs, Operand, Place, Rvalue, Statement, StatementKind, TerminatorKind};

use super::forall_gpu::needs_wire_conversion;
use super::{lower_expression, LoweringContext};

/// Aggregated result of analyzing GPU function arguments for a kernel launch.
pub(super) struct ThreadedGpuFnArgs {
    pub(super) kernel_op: Operand,
    pub(super) kernel_name: String,
    pub(super) args: GpuFnArgs,
}

/// The arguments of a `gpu fn` call, split the way a launch binds them: each
/// buffer with its device handle, access and wire conversion, and each scalar
/// packed into the kernel's scalar-input uniform, all in argument order.
#[derive(Default)]
pub(super) struct GpuFnArgs {
    pub(super) buffer_args: Vec<Operand>,
    pub(super) arg_handles: Vec<Option<crate::mir::body::DeviceHandleId>>,
    pub(super) arg_read_only: Vec<bool>,
    pub(super) arg_int_narrow: Vec<bool>,
    pub(super) scalar_args: Vec<Operand>,
}

/// Lowers the arguments of a call to the `gpu fn` `func_name`: a buffer must be
/// a gpu-resident place, and a scalar is any type the GPU wire format gives a
/// uniform lane ([`scalar_capture_wire`]). Any other argument is an internal
/// error rather than a silently dropped binding: the type checker refuses a
/// scalar parameter with no 32-bit lane.
// TODO: a vector parameter (`Vec3<f32>`) is admitted by the type checker but
// has no launch binding yet, so launching such a `gpu fn` reaches the internal
// error below; it needs either a uniform/storage binding or a refusal at the
// signature.
pub(super) fn process_gpu_fn_args(
    ctx: &mut LoweringContext,
    func_name: &str,
    call_args: &[Expression],
    span: Span,
) -> Result<GpuFnArgs, LoweringError> {
    let out_params = ctx
        .type_checker
        .function_out_params()
        .get(func_name)
        .cloned()
        .unwrap_or_default();

    let mut args = GpuFnArgs::default();
    for (arg_idx, arg) in call_args.iter().enumerate() {
        let arg_ty = ctx
            .type_checker
            .get_type(arg.id)
            .cloned()
            .unwrap_or_else(|| Type::new(TypeKind::Void, span));
        let arg_op = lower_expression(ctx, arg, None)?;

        if is_gpu_buffer_type(&arg_ty.kind) {
            let is_out = out_params.get(arg_idx).copied().unwrap_or(false);
            push_buffer_arg(ctx, &mut args, arg_op, &arg_ty, is_out, span)?;
        } else if scalar_capture_wire(&arg_ty.kind).is_some() {
            let scalar = scalar_arg_place(ctx, arg_op, arg_ty, span);
            args.scalar_args.push(scalar);
        } else {
            return Err(LoweringError::internal(
                DiagnosticCode::MirGpuLaunchMetadataMismatch,
                format!(
                    "argument {arg_idx} of the launch of '{func_name}' has type '{arg_ty}', \
                     which binds neither as a buffer nor as a scalar input"
                ),
                arg.span,
            ));
        }
    }
    Ok(args)
}

/// A scalar launch argument as a projection-free local, the form the launch
/// reads its scalar inputs from: a constant or projected value is first
/// copied into a temporary of its type.
fn scalar_arg_place(
    ctx: &mut LoweringContext,
    arg_op: Operand,
    arg_ty: Type,
    span: Span,
) -> Operand {
    if let Operand::Copy(place) | Operand::Move(place) = &arg_op {
        if place.projection.is_empty() {
            return arg_op;
        }
    }
    let temp = ctx.push_temp(arg_ty, span);
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(Place::new(temp), Rvalue::Use(arg_op)),
        span,
    });
    Operand::Copy(Place::new(temp))
}

/// Records one buffer argument, refusing a host-resident or non-place buffer.
fn push_buffer_arg(
    ctx: &LoweringContext,
    args: &mut GpuFnArgs,
    arg_op: Operand,
    arg_ty: &Type,
    is_out: bool,
    span: Span,
) -> Result<(), LoweringError> {
    let (Operand::Copy(place) | Operand::Move(place)) = &arg_op else {
        return Err(LoweringError::unsupported_expression(
            "gpu fn buffer args must be places".to_string(),
            span,
        ));
    };
    let local_decl = &ctx.body.local_decls[place.local.0];
    if !matches!(
        local_decl.residency,
        crate::mir::body::BindingResidency::Gpu
    ) {
        let buffer_name = local_decl.name.as_deref().unwrap_or("argument");
        return Err(LoweringError::coded(
            DiagnosticCode::TypGpuFunctionHostBufferMismatch,
            format!(
                "cannot pass host-resident array '{}' to gpu function",
                buffer_name
            ),
            span,
            Some(format!(
                "mark the binding as gpu-resident: 'gpu let {} = ...' or 'gpu var {} = ...'",
                buffer_name, buffer_name
            )),
        ));
    }

    args.arg_handles.push(local_decl.device_handle);
    args.arg_read_only.push(!is_out);
    args.arg_int_narrow.push(needs_wire_conversion(arg_ty));
    args.buffer_args.push(arg_op);
    Ok(())
}

/// Analyze GPU function arguments for a kernel launch, producing operands and metadata.
pub(super) fn thread_gpu_fn_args(
    ctx: &mut LoweringContext,
    callee: &Expression,
    call_args: &[Expression],
    span: Span,
) -> Result<ThreadedGpuFnArgs, LoweringError> {
    let (kernel_op, kernel_name) = super::dispatch::resolve_kernel_operand(ctx, callee, span)?;

    let ExpressionKind::Identifier(func_name, _) = &callee.node else {
        return Err(LoweringError::unsupported_expression(
            "gpu fn must be called by name".to_string(),
            span,
        ));
    };

    let args = process_gpu_fn_args(ctx, func_name, call_args, span)?;
    Ok(ThreadedGpuFnArgs {
        kernel_op,
        kernel_name,
        args,
    })
}

fn is_gpu_buffer_type(kind: &TypeKind) -> bool {
    match kind {
        TypeKind::Array(_, _) | TypeKind::List(_) => true,
        TypeKind::Custom(n, _) => super::dispatch::is_collection_type(n),
        _ => false,
    }
}

/// Try to extract Dim3(x, y, z) as [x, y, z] from a compile-time literal.
/// Returns None if the expression is not a Dim3 literal or is not compile-time constant.
pub(super) fn try_extract_dim3_literal(expr: &Expression) -> Option<[u32; 3]> {
    use crate::ast::expression::ExpressionKind;

    match &expr.node {
        ExpressionKind::Call(func, args) => {
            if let ExpressionKind::Identifier(name, _) = &func.node {
                if name == "Dim3" && args.len() == 3 {
                    let x = extract_u32_literal(&args[0])?;
                    let y = extract_u32_literal(&args[1])?;
                    let z = extract_u32_literal(&args[2])?;
                    return Some([x, y, z]);
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_u32_literal(expr: &Expression) -> Option<u32> {
    use crate::ast::expression::ExpressionKind;
    use crate::ast::literal::Literal;

    match &expr.node {
        ExpressionKind::Literal(Literal::Integer(int_lit)) => {
            use crate::ast::literal::IntegerLiteral;
            match int_lit {
                IntegerLiteral::I8(v) if *v >= 0 => Some(*v as u32),
                IntegerLiteral::I16(v) if *v >= 0 => Some(*v as u32),
                IntegerLiteral::I32(v) if *v >= 0 => Some(*v as u32),
                IntegerLiteral::I64(v) if *v >= 0 => Some(*v as u32),
                IntegerLiteral::U8(v) => Some(*v as u32),
                IntegerLiteral::U16(v) => Some(*v as u32),
                IntegerLiteral::U32(v) => Some(*v),
                IntegerLiteral::U64(v) if *v <= u32::MAX as u64 => Some(*v as u32),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Lower a GPU kernel launch: `kernel_handle.launch(grid, block)`.
pub(crate) fn try_lower_kernel_launch(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    obj: &Expression,
    prop: &Expression,
    args: &[Expression],
    dest: Option<crate::mir::Place>,
) -> Result<Option<Operand>, LoweringError> {
    let ExpressionKind::Identifier(name, _) = &prop.node else {
        return Ok(None);
    };
    if name != "launch" || !receiver_is_kernel(ctx, obj) {
        return Ok(None);
    }

    if args.len() != 2 {
        return Err(LoweringError::invalid_gpu_launch_args(2, args.len(), *span));
    }
    let dimension_watermark = ctx.body.local_decls.len();
    let grid_op = lower_expression(ctx, &args[0], None)?;
    let block_op = lower_expression(ctx, &args[1], None)?;
    let dimension_locals: Vec<crate::mir::Local> = [&grid_op, &block_op]
        .iter()
        .filter_map(|op| match op {
            Operand::Copy(place) | Operand::Move(place) => Some(place.local),
            Operand::Constant(_) => None,
        })
        .collect();

    let return_ty = ctx
        .type_checker
        .get_type(call_expr_id)
        .cloned()
        .unwrap_or_else(|| Type::new(TypeKind::Void, *span));
    let (destination, op) = super::dispatch::call_destination(ctx, return_ty, dest, *span);
    let target_bb = ctx.new_basic_block();

    let (kernel_op, kernel_name, gpu_args) =
        if let ExpressionKind::Call(callee, call_args) = &obj.node {
            let threaded = thread_gpu_fn_args(ctx, callee, call_args, *span)?;
            (
                threaded.kernel_op,
                Some(threaded.kernel_name),
                threaded.args,
            )
        } else {
            (
                lower_expression(ctx, obj, None)?,
                None,
                GpuFnArgs::default(),
            )
        };

    if let Some(ref kernel_name) = kernel_name {
        let workgroup_size = try_extract_dim3_literal(&args[1]).ok_or_else(|| {
            LoweringError::coded(
                DiagnosticCode::TypGpuLaunchBlockSizeNotLiteral,
                "gpu fn launch block size must be a compile-time literal Dim3".to_string(),
                *span,
                Some("use a compile-time literal, e.g., block: Dim3(16, 16, 1)".to_string()),
            )
        })?;

        if workgroup_size.contains(&0) {
            return Err(LoweringError::coded(
                DiagnosticCode::TypGpuLaunchBlockDimensionsInvalid,
                "gpu fn launch block dimensions must all be >0".to_string(),
                *span,
                Some("each dimension must be at least 1".to_string()),
            ));
        }

        ctx.body
            .kernel_workgroups
            .push((kernel_name.clone(), workgroup_size));
        if let Some(grid) = try_extract_dim3_literal(&args[0]) {
            ctx.body.kernel_grids.push((kernel_name.clone(), grid));
        }
    }

    let GpuFnArgs {
        buffer_args,
        arg_handles,
        arg_read_only,
        arg_int_narrow,
        scalar_args,
    } = gpu_args;
    let launch_args = GpuLaunchArgs::new(buffer_args, arg_handles, arg_read_only, arg_int_narrow)
        .map_err(|e| {
        LoweringError::internal(DiagnosticCode::MirGpuLaunchMetadataMismatch, e, *span)
    })?;

    ctx.set_terminator(crate::mir::Terminator::new(
        TerminatorKind::GpuLaunch {
            kernel: kernel_op,
            grid: grid_op,
            block: block_op,
            launch_args,
            scalar_args,
            uniform_bound_x: None,
            uniform_bound_y: None,
            uniform_bound_z: None,
            uniform_start_x: None,
            uniform_start_y: None,
            uniform_start_z: None,
            destination,
            target: Some(target_bb),
        },
        *span,
    ));
    ctx.set_current_block(target_bb);
    // The grid and block dimensions are allocations of their own, read by the
    // launch and dead once it returns. A local the caller named is older than
    // the watermark and keeps its own release.
    for local in dimension_locals {
        ctx.emit_temp_drop(local, dimension_watermark, *span);
    }
    Ok(Some(op))
}

/// True when `obj` has the GPU `Kernel` type.
fn receiver_is_kernel(ctx: &LoweringContext, obj: &Expression) -> bool {
    ctx.type_checker
        .get_type(obj.id)
        .map(|ty| matches!(&ty.kind, TypeKind::Custom(n, _) if n == "Kernel"))
        .unwrap_or(false)
}
