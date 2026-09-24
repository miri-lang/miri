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

use super::helpers::{coerce_rvalue_in, release_coerced_source};
use super::{lower_expression, resolve_type, LoweringContext};
use crate::error::lowering::LoweringError;

/// Runtime entry that frees the device buffer of a handle's innermost
/// activation and closes that activation. Synthesized by the compiler like the
/// other entries in [`crate::mir::residency`]; codegen declares the import on
/// demand.
const RELEASE_FN: &str = "miri_gpu_release";

/// Fence outstanding device writes and copy the gpu binding `initializer` names
/// back to its host value, for a read the readback pass cannot see: one that
/// hands the binding to a runtime call as an argument, as `g.slice(range)` does.
/// Any other expression, and a host binding, emits nothing.
pub(crate) fn emit_cross_residency_readback(
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
    ctx.current_block =
        crate::mir::residency::append_readback(&mut ctx.body, ctx.current_block, src_local, span);
}

/// Opens a fresh activation of `handle`, so this execution of a `gpu`
/// binding's declaration starts with no device buffer — its first launch
/// uploads the host value — while an enclosing activation of the same binding
/// (a recursive caller) keeps its own buffer.
pub(crate) fn emit_gpu_activation(ctx: &mut LoweringContext, handle: DeviceHandleId, span: Span) {
    emit_void_runtime_call(
        ctx,
        crate::mir::residency::ACQUIRE_FN,
        vec![handle_operand(handle, span)],
        span,
    );
}

/// Hands the live activation of handle `from`, and the device buffer it holds,
/// to handle `to`, and reopens `from` with no buffer. Codegen closes the moved
/// activation when the binding carrying `to` leaves scope.
fn emit_gpu_transfer(
    ctx: &mut LoweringContext,
    from: DeviceHandleId,
    to: DeviceHandleId,
    span: Span,
) {
    emit_void_runtime_call(
        ctx,
        crate::mir::residency::TRANSFER_FN,
        vec![handle_operand(from, span), handle_operand(to, span)],
        span,
    );
}

/// Frees the device buffer of `handle`'s innermost activation and closes it,
/// for a binding that stops referring to that handle before its scope ends.
pub(crate) fn emit_gpu_release(ctx: &mut LoweringContext, handle: DeviceHandleId, span: Span) {
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
            arg_handles: Vec::new(),
            destination: Place::new(dest_local),
            target: Some(after_bb),
        },
        span,
    ));
    ctx.set_current_block(after_bb);
}

/// The type an alias name ultimately stands for.
///
/// A declared type reaches lowering spelled the way it was written, so
/// `type Meters is int` arrives as `Custom("Meters")`. Left that way, every
/// decision that follows reads an opaque name rather than the type behind it:
/// the coercion the initializer needs, the storage the value gets, and whether
/// it is reference counted. Aliases may chain, so the walk continues until it
/// reaches a type that is not itself an alias; a cycle stops it and yields the
/// type as written.
fn resolve_alias_target(tc: &crate::type_checker::TypeChecker, ty: &Type) -> Type {
    let mut current = ty.clone();
    let mut visited = std::collections::HashSet::new();
    while let TypeKind::Custom(name, _) = &current.kind {
        if !visited.insert(name.clone()) {
            return ty.clone();
        }
        let Some(crate::type_checker::context::TypeDefinition::Alias(alias)) =
            tc.type_definitions().get(name.as_str())
        else {
            break;
        };
        current = alias.template.clone();
    }
    current
}

/// A declared type in the form every later pass expects.
///
/// A type reaches lowering spelled the way it was written, while the rest of
/// the pipeline reads the canonical form that inference produces. Two spellings
/// diverge: an alias stands in for the type behind it, and an optional written
/// as a generic argument keeps its payload inside a type-argument expression
/// where nothing looks for it. Both leave a later pass reading a name instead of
/// a type — which storage to give the value, which coercion its initializer
/// needs, and whether the value is reference counted all then answer wrongly.
pub(crate) fn canonical_declared_type(tc: &crate::type_checker::TypeChecker, ty: &Type) -> Type {
    let resolved = resolve_alias_target(tc, ty);
    let TypeKind::Custom(name, Some(args)) = &resolved.kind else {
        return resolved;
    };
    if name == crate::ast::types::OPTION_TYPE_NAME && args.len() == 1 {
        let payload = canonical_declared_type(tc, &resolve_type(tc, &args[0]));
        return Type::new(TypeKind::Option(Box::new(payload)), resolved.span);
    }
    let canonical_args = args
        .iter()
        .map(|arg| canonical_type_argument(tc, arg))
        .collect();
    Type::new(
        TypeKind::Custom(name.clone(), Some(canonical_args)),
        resolved.span,
    )
}

/// One type argument of a declared generic type, in canonical form.
///
/// A type argument reaches lowering as an expression, and the nullable half of
/// `int?` rides on that expression rather than on the type inside it. Readers
/// that take the inner type alone — the element-drop path among them — then see
/// a bare `int` and treat the element as a value with nothing to release.
/// Folding the flag into the type is what makes `[int?]` and `[Option<int>]`
/// the single type they are meant to be. Arguments that are not types, such as
/// the size in `[T; N]`, are carried through untouched.
fn canonical_type_argument(tc: &crate::type_checker::TypeChecker, arg: &Expression) -> Expression {
    let ExpressionKind::Type(ty, is_nullable) = &arg.node else {
        return arg.clone();
    };
    let inner = canonical_declared_type(tc, ty);
    let canonical = if *is_nullable {
        Type::new(TypeKind::Option(Box::new(inner)), ty.span)
    } else {
        inner
    };
    Expression::new(
        arg.id,
        ExpressionKind::Type(Box::new(canonical), false),
        arg.span,
    )
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
        // A written type is the one spelling of a local's type that the type
        // checker never rewrites: `var x Tagged<T>` still names the enclosing
        // body's parameter. The instantiation's substitution has to be applied
        // here, or the local is released as `Tagged<T>` — whose field is a bare
        // parameter, so the shared drop sees nothing managed to release.
        return Ok((
            ctx.declared_type(type_expr),
            decl.initializer.as_deref(),
            None,
        ));
    }
    let Some(init_expr) = decl.initializer.as_deref() else {
        return Err(LoweringError::unsupported_expression(
            format!("Cannot determine type for variable '{}'", decl.name),
            *span,
        ));
    };
    if let Some(ty) = ctx.recorded_type(init_expr.id) {
        return Ok((ty, Some(init_expr), None));
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
        lower_single_variable(ctx, decl, span)?;
    }
    Ok(())
}

/// Lower one variable declaration: resolve its type, allocate the local, apply
/// residency metadata, and lower the initializer.
fn lower_single_variable(
    ctx: &mut LoweringContext,
    decl: &VariableDeclaration,
    span: &Span,
) -> Result<(), LoweringError> {
    let (var_ty, init_expr_opt, pre_lowered_op) = resolve_decl_init(ctx, decl, span)?;
    let var_ty_kind = var_ty.kind.clone();
    // Allocate the local but defer binding its name: a shadowing initializer
    // (`let x = x + 1`) must resolve `x` to the outer binding, not the local we
    // are declaring. The name becomes resolvable only after the initializer is
    // lowered.
    let local = ctx.alloc_local(decl.name.clone(), var_ty, *span);
    ctx.body.local_decls[local.0].name_span = decl.name_span;

    apply_variable_residency(ctx, local, decl, span);

    if let Some(init_expr) = init_expr_opt {
        assign_variable_initializer(ctx, local, init_expr, pre_lowered_op, &var_ty_kind, span)?;
    }
    ctx.bind_local_name(decl.name.clone(), local);
    Ok(())
}

/// Apply shared-storage and host/gpu residency metadata to a freshly-declared
/// local, allocating a device handle (and opening its activation) for gpu vars.
fn apply_variable_residency(
    ctx: &mut LoweringContext,
    local: crate::mir::Local,
    decl: &VariableDeclaration,
    span: &Span,
) {
    if decl.is_shared {
        ctx.body.local_decls[local.0].storage_class = StorageClass::GpuShared;
    }
    ctx.body.local_decls[local.0].residency = match decl.residency {
        AstResidency::Host => MirResidency::Host,
        AstResidency::Gpu => MirResidency::Gpu,
    };
    if ctx.body.local_decls[local.0].residency != MirResidency::Gpu {
        return;
    }
    match gpu_move_source_handle(ctx, decl) {
        // A move out of a borrowed parameter shares the caller's handle and
        // stays borrowed: the caller still owns the buffer, and a parameter is
        // never assigned a new value that could land in it.
        Some((handle, true)) => {
            ctx.body.local_decls[local.0].device_handle = Some(handle);
            ctx.body.local_decls[local.0].device_handle_borrowed = true;
        }
        // `gpu let/var b = a` where `a` is a gpu-resident binding is a move (the
        // type checker has consumed `a`): `b` takes over `a`'s live activation
        // and the device buffer it holds, so `b`'s first launch reuses the
        // already-uploaded buffer. `b` gets a handle of its own rather than
        // sharing `a`'s, because `a` may be assigned a new value afterwards and
        // that value is uploaded into `a`'s handle — which the transfer leaves
        // with a fresh activation and no buffer, not the one `b` now holds.
        Some((source, false)) => {
            let handle = ctx.fresh_device_handle();
            ctx.body.local_decls[local.0].device_handle = Some(handle);
            emit_gpu_transfer(ctx, source, handle, *span);
        }
        None => {
            let handle = ctx.fresh_device_handle();
            ctx.body.local_decls[local.0].device_handle = Some(handle);
            emit_gpu_activation(ctx, handle, *span);
        }
    }
}

/// Device handle of a gpu-to-gpu move source — a bare identifier initializer
/// bound to a gpu-resident local with a live device handle — and whether the
/// source only borrows it. When target is gpu-resident and source is a gpu
/// binding, the moved binding inherits the source buffer instead of allocating
/// a fresh one.
fn gpu_move_source_handle(
    ctx: &LoweringContext,
    decl: &VariableDeclaration,
) -> Option<(DeviceHandleId, bool)> {
    let Expression {
        node: ExpressionKind::Identifier(name, _),
        ..
    } = decl.initializer.as_deref()?
    else {
        return None;
    };
    let src_local = *ctx.variable_map.get(name.as_str())?;
    let src_decl = &ctx.body.local_decls[src_local.0];
    if src_decl.residency != MirResidency::Gpu {
        return None;
    }
    src_decl
        .device_handle
        .map(|handle| (handle, src_decl.device_handle_borrowed))
}

/// Lower a variable's initializer into `local`: assign a pre-lowered operand,
/// use DPS when the types match, or fall back to a temp + cast/assign.
fn assign_variable_initializer(
    ctx: &mut LoweringContext,
    local: crate::mir::Local,
    init_expr: &Expression,
    pre_lowered_op: Option<Operand>,
    var_ty_kind: &TypeKind,
    span: &Span,
) -> Result<(), LoweringError> {
    let dest = Place::new(local);
    if let Some(op) = pre_lowered_op {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(dest, Rvalue::Use(op)),
            span: *span,
        });
        return Ok(());
    }

    let init_ty = ctx.recorded_type(init_expr.id);
    let types_match = init_ty.as_ref().is_some_and(|ity| {
        MirType::from_type_kind(&ity.kind) == MirType::from_type_kind(var_ty_kind)
    });
    if types_match {
        lower_expression(ctx, init_expr, Some(dest))?;
        return Ok(());
    }

    let watermark = ctx.body.local_decls.len();
    let op = lower_expression(ctx, init_expr, None)?;
    let op_ty = op.ty(&ctx.body).clone();
    let target_ty = ctx.body.local_decls[local.0].ty.clone();
    let rvalue = if op_ty.kind != *var_ty_kind {
        coerce_rvalue_in(ctx, op.clone(), &op_ty, &target_ty, *span)
    } else {
        Rvalue::Use(op.clone())
    };
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(dest, rvalue),
        span: *span,
    });
    release_coerced_source(ctx, &op, &op_ty, &target_ty, watermark, *span);
    Ok(())
}
