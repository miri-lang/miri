// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::statement::{BindingResidency as AstResidency, VariableDeclaration};
use crate::ast::types::{Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::body::{BindingResidency as MirResidency, DeviceHandleId};
use crate::mir::types::MirType;
use crate::mir::{
    Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind, StorageClass, Terminator,
    TerminatorKind,
};

use super::{helpers::coerce_rvalue, lower_expression, resolve_type, LoweringContext};
use crate::error::lowering::LoweringError;

// These two GPU intrinsics are synthesized by the compiler, never written in
// Miri source, so they are not declared as `runtime "gpu" fn` in any `.mi`
// (their device-handle / array-header arguments are not expressible Miri
// types). Like `miri_gpu_launch_inline`, codegen declares the import on
// demand from the emitted call's operands.

/// Runtime entry that fences outstanding device writes and copies a
/// `gpu`-resident buffer back to its host array.
const READBACK_FN: &str = "miri_gpu_readback";

/// Runtime entry that drops the persistent device buffer owned by a handle.
const RELEASE_FN: &str = "miri_gpu_release";

/// When a host binding is initialized directly from a `gpu`-resident
/// identifier (`let h = g`), emit the cross-residency readback before the
/// copy so `h` observes the device-side results. This is the only point that
/// fences device work; reuse and launch never do.
///
/// Modeled as a borrowing call: the array is passed by `Copy` (no Perceus
/// IncRef on terminator operands), so the gpu binding survives the readback
/// and remains available for a second readback.
fn emit_cross_residency_readback(
    ctx: &mut LoweringContext,
    initializer: Option<&Expression>,
    span: Span,
) {
    let Some(Expression {
        node: ExpressionKind::Identifier(name, _),
        ..
    }) = initializer
    else {
        return;
    };
    let Some(&src_local) = ctx.variable_map.get(name.as_str()) else {
        return;
    };
    let Some(handle) = ctx.body.local_decls[src_local.0].device_handle else {
        return;
    };

    let array_op = Operand::Copy(Place::new(src_local));
    emit_void_runtime_call(
        ctx,
        READBACK_FN,
        vec![handle_operand(handle, span), array_op],
        span,
    );
}

/// Releases any device buffer left over from a prior runtime lifetime of this
/// handle so a re-declared `gpu` binding (e.g. a binding in a function called
/// more than once) starts fresh: its first launch re-uploads rather than
/// reusing stale device bytes. A noop the first time a handle is declared.
fn emit_gpu_buffer_reset(ctx: &mut LoweringContext, handle: DeviceHandleId, span: Span) {
    emit_void_runtime_call(ctx, RELEASE_FN, vec![handle_operand(handle, span)], span);
}

fn handle_operand(handle: DeviceHandleId, span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Int, span),
        literal: Literal::Integer(IntegerLiteral::I64(handle.0 as i64)),
    }))
}

/// Emits a borrowing call to a runtime entry, splitting the current block.
/// Borrowing because terminator-operand copies are not IncRef'd by Perceus,
/// so any managed argument survives the call. The destination is a `void`
/// temp, so any status the entry returns is intentionally discarded —
/// failures surface through the runtime's own log, not the program.
fn emit_void_runtime_call(
    ctx: &mut LoweringContext,
    fn_name: &str,
    args: Vec<Operand>,
    span: Span,
) {
    let func = Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: Literal::Identifier(fn_name.to_string()),
    }));
    let dest_local = ctx.push_temp(Type::new(TypeKind::Void, span), span);
    let after_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func,
            args,
            out_args: Vec::new(),
            destination: Place::new(dest_local),
            target: Some(after_bb),
        },
        span,
    ));
    ctx.set_current_block(after_bb);
}

/// Resolves a declaration's type and initializer operand. Returns the
/// variable type, the initializer expression (borrowed from `decl`), and an
/// already-lowered operand when type inference forced an early lowering.
fn resolve_decl_init<'d>(
    ctx: &mut LoweringContext,
    decl: &'d VariableDeclaration,
    span: &Span,
) -> Result<(Type, Option<&'d Expression>, Option<Operand>), LoweringError> {
    if let Some(type_expr) = &decl.typ {
        let ty = resolve_type(ctx.type_checker, type_expr);
        return Ok((ty, decl.initializer.as_deref(), None));
    }
    let Some(init_expr) = decl.initializer.as_deref() else {
        return Err(LoweringError::unsupported_expression(
            format!("Cannot determine type for variable '{}'", decl.name),
            *span,
        ));
    };
    if let Some(ty) = ctx.type_checker.get_type(init_expr.id) {
        return Ok((ty.clone(), Some(init_expr), None));
    }
    // No recorded type: lower now to infer it.
    let op = lower_expression(ctx, init_expr, None)?;
    let ty = op.ty(&ctx.body).clone();
    Ok((ty, Some(init_expr), Some(op)))
}

pub fn lower_variable(
    ctx: &mut LoweringContext,
    decls: &[VariableDeclaration],
    span: &Span,
) -> Result<(), LoweringError> {
    for decl in decls {
        if decl.residency == AstResidency::Host {
            emit_cross_residency_readback(ctx, decl.initializer.as_deref(), *span);
        }
        let (var_ty, init_expr_opt, pre_lowered_op) = resolve_decl_init(ctx, decl, span)?;

        // Clone var_ty only when needed for comparison; consume in final use
        let var_ty_kind = var_ty.kind.clone();
        let local = ctx.push_local(decl.name.clone(), var_ty, *span);

        if decl.is_shared {
            ctx.body.local_decls[local.0].storage_class = StorageClass::GpuShared;
        }

        ctx.body.local_decls[local.0].residency = match decl.residency {
            AstResidency::Host => MirResidency::Host,
            AstResidency::Gpu => MirResidency::Gpu,
        };
        if ctx.body.local_decls[local.0].residency == MirResidency::Gpu {
            let handle = DeviceHandleId::fresh();
            ctx.body.local_decls[local.0].device_handle = Some(handle);
            emit_gpu_buffer_reset(ctx, handle, *span);
        }

        if let Some(init_expr) = init_expr_opt {
            let dest = Place::new(local);

            // If we already lowered it (inference case), we must assign it now
            if let Some(op) = pre_lowered_op {
                // op.ty() == var_ty, so no cast needed
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(dest, Rvalue::Use(op)),
                    span: *span,
                });
            } else {
                // Check if we can use DPS (types match)
                let init_ty = ctx.type_checker.get_type(init_expr.id);
                // Comparison: ignore spans
                let types_match = init_ty.is_some_and(|ity| {
                    MirType::from_type_kind(&ity.kind) == MirType::from_type_kind(&var_ty_kind)
                });

                if types_match {
                    // Optimized path: write directly to variable
                    lower_expression(ctx, init_expr, Some(dest))?;
                } else {
                    // Fallback: create temp/use result, then cast/assign
                    let op = lower_expression(ctx, init_expr, None)?;
                    let op_ty = op.ty(&ctx.body).clone();

                    let rvalue = if op_ty.kind != var_ty_kind {
                        let target_ty = ctx.body.local_decls[local.0].ty.clone();
                        coerce_rvalue(op, &op_ty, &target_ty)
                    } else {
                        Rvalue::Use(op)
                    };

                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(dest, rvalue),
                        span: *span,
                    });
                }
            }
        }
    }
    Ok(())
}
