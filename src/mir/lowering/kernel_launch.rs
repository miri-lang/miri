// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU kernel launch lowering.

use crate::ast::expression::Expression;
use crate::ast::{ExpressionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{GpuLaunchArgs, Operand, TerminatorKind};

use super::{lower_expression, LoweringContext};

/// Aggregated result of analyzing GPU function arguments for a kernel launch.
pub(super) struct ThreadedGpuFnArgs {
    pub(super) kernel_op: Operand,
    pub(super) kernel_name: String,
    pub(super) buffer_args: Vec<Operand>,
    pub(super) arg_handles: Vec<Option<crate::mir::body::DeviceHandleId>>,
    pub(super) arg_read_only: Vec<bool>,
    pub(super) arg_int_narrow: Vec<bool>,
    pub(super) scalar_args: Vec<Operand>,
}

/// Process buffer arguments and metadata for a GPU function call.
#[allow(clippy::type_complexity)]
pub(super) fn process_gpu_buffer_args(
    ctx: &mut LoweringContext,
    func_name: &str,
    call_args: &[Expression],
    span: Span,
) -> Result<
    (
        Vec<Operand>,
        Vec<Option<crate::mir::body::DeviceHandleId>>,
        Vec<bool>,
        Vec<bool>,
    ),
    LoweringError,
> {
    let out_params = ctx
        .type_checker
        .function_out_params()
        .get(func_name)
        .cloned()
        .unwrap_or_default();

    let mut buffer_args = Vec::new();
    let mut arg_handles = Vec::new();
    let mut arg_read_only = Vec::new();
    let mut arg_int_narrow = Vec::new();

    for (arg_idx, arg) in call_args.iter().enumerate() {
        let arg_ty = ctx
            .type_checker
            .get_type(arg.id)
            .cloned()
            .unwrap_or_else(|| Type::new(TypeKind::Void, span));
        let arg_op = lower_expression(ctx, arg, None)?;

        if is_gpu_buffer_type(&arg_ty.kind) {
            if let Operand::Copy(place) | Operand::Move(place) = &arg_op {
                let local_decl = &ctx.body.local_decls[place.local.0];

                if !matches!(
                    local_decl.residency,
                    crate::mir::body::BindingResidency::Gpu
                ) {
                    let buffer_name = local_decl.name.as_deref().unwrap_or("argument");
                    return Err(LoweringError::custom(
                        format!("cannot pass host-resident array '{}' to gpu function", buffer_name),
                        span,
                        Some(format!(
                            "mark the binding as gpu-resident: 'gpu let {} = ...' or 'gpu var {} = ...'",
                            buffer_name, buffer_name
                        )),
                    ));
                }

                let handle = local_decl.device_handle;
                arg_handles.push(handle);
                buffer_args.push(arg_op.clone());

                arg_read_only.push(!out_params.get(arg_idx).copied().unwrap_or(false));
                arg_int_narrow.push(needs_int_narrowing(&arg_ty));
            } else {
                return Err(LoweringError::unsupported_expression(
                    "gpu fn buffer args must be places".to_string(),
                    span,
                ));
            }
        }
    }

    Ok((buffer_args, arg_handles, arg_read_only, arg_int_narrow))
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

    let (buffer_args, arg_handles, arg_read_only, arg_int_narrow) =
        process_gpu_buffer_args(ctx, func_name, call_args, span)?;

    Ok(ThreadedGpuFnArgs {
        kernel_op,
        kernel_name,
        buffer_args,
        arg_handles,
        arg_read_only,
        arg_int_narrow,
        scalar_args: Vec::new(),
    })
}

fn is_gpu_buffer_type(kind: &TypeKind) -> bool {
    match kind {
        TypeKind::Array(_, _) | TypeKind::List(_) => true,
        TypeKind::Custom(n, _) => super::dispatch::is_collection_type(n),
        _ => false,
    }
}

fn needs_int_narrowing(ty: &Type) -> bool {
    use super::forall_gpu::needs_int_narrowing as check_narrowing;
    check_narrowing(ty)
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

    let (
        kernel_op,
        kernel_name,
        call_args,
        arg_handles,
        arg_read_only,
        arg_int_narrow,
        scalar_args,
    ) = if let ExpressionKind::Call(callee, call_args) = &obj.node {
        let gpu_args = thread_gpu_fn_args(ctx, callee, call_args, *span)?;
        (
            gpu_args.kernel_op,
            Some(gpu_args.kernel_name),
            gpu_args.buffer_args,
            gpu_args.arg_handles,
            gpu_args.arg_read_only,
            gpu_args.arg_int_narrow,
            gpu_args.scalar_args,
        )
    } else {
        (
            lower_expression(ctx, obj, None)?,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    };

    if let Some(ref kernel_name) = kernel_name {
        let workgroup_size = try_extract_dim3_literal(&args[1]).ok_or_else(|| {
            LoweringError::custom(
                "gpu fn launch block size must be a compile-time literal Dim3".to_string(),
                *span,
                Some("use a compile-time literal, e.g., block: Dim3(16, 16, 1)".to_string()),
            )
        })?;

        if workgroup_size.contains(&0) {
            return Err(LoweringError::custom(
                "gpu fn launch block dimensions must all be >0".to_string(),
                *span,
                Some("each dimension must be at least 1".to_string()),
            ));
        }

        ctx.body
            .kernel_workgroups
            .push((kernel_name.clone(), workgroup_size));
    }

    let launch_args = GpuLaunchArgs::new(call_args, arg_handles, arg_read_only, arg_int_narrow)
        .map_err(|e| LoweringError::custom(e.to_string(), *span, None))?;

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
