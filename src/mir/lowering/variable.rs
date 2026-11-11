// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::statement::{BindingResidency as AstResidency, VariableDeclaration};
use crate::ast::type_arguments::TypeArgument;
use crate::ast::type_arguments::{rewritten, TypeArgKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::body::{BindingResidency as MirResidency, DeviceHandleId};
use crate::mir::option_layout::resolve_alias;
use crate::mir::types::MirType;
use crate::mir::{
    Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind, StorageClass, Terminator,
    TerminatorKind,
};

use super::helpers::{coerce_rvalue_in, release_coerced_source};
use super::{lower_expression, resolve_argument_type, LoweringContext};
use crate::error::lowering::LoweringError;

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
        &crate::mir::compiler_intrinsics::ACQUIRE,
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
        &crate::mir::compiler_intrinsics::TRANSFER,
        vec![handle_operand(from, span), handle_operand(to, span)],
        span,
    );
}

/// Frees the device buffer of `handle`'s innermost activation and closes it,
/// for a binding that stops referring to that handle before its scope ends.
pub(crate) fn emit_gpu_release(ctx: &mut LoweringContext, handle: DeviceHandleId, span: Span) {
    emit_void_runtime_call(
        ctx,
        &crate::mir::compiler_intrinsics::RELEASE,
        vec![handle_operand(handle, span)],
        span,
    );
}

fn handle_operand(handle: DeviceHandleId, span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Int, span),
        literal: Literal::Integer(IntegerLiteral::I64(handle.0 as i64)),
    }))
}

/// Emits a borrowing call to a runtime entry, splitting the current block.
/// Borrowing because terminator-operand copies are not IncRef'd by RC insertion,
/// so any managed argument survives the call. The destination is a `void`
/// temp, so any status the entry returns is intentionally discarded —
/// failures surface through the runtime's own log, not the program.
fn emit_void_runtime_call(
    ctx: &mut LoweringContext,
    intrinsic: &crate::mir::compiler_intrinsics::Intrinsic,
    args: Vec<Operand>,
    span: Span,
) {
    let func = Operand::runtime(intrinsic.callee(), span);
    let dest_local = ctx.push_temp(Type::new(TypeKind::Void, span), span);
    let after_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func,
            args,
            arg_modes: Vec::new(),
            arg_handles: Vec::new(),
            flags: crate::mir::CallFlags::default(),
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
/// it is reference counted. A generic alias stands for its template at the
/// arguments it is written with (`Opt<Node>` for `type Opt<T> is T?` is
/// `Node?`), and aliases may chain; [`resolve_alias`] reads both, the same way
/// the optional layout and the release path do. A cycle yields the type as
/// written.
fn resolve_alias_target(tc: &crate::type_checker::TypeChecker, ty: &Type) -> Type {
    resolve_alias(&ty.kind, tc.type_definitions()).unwrap_or_else(|| ty.clone())
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
    // A tuple's elements carry their nullable flag the way a generic
    // argument does, so each is folded the same way: left as a flag, the
    // drop path reads `(int?, String)` as holding a bare `int` and never
    // releases the optional's box.
    if let TypeKind::Tuple(elements) = &resolved.kind {
        let canonical_elements = elements
            .iter()
            .map(|element| canonical_type_argument(tc, element))
            .collect();
        return Type::new(TypeKind::Tuple(canonical_elements), resolved.span);
    }
    let TypeKind::Custom(name, Some(args)) = &resolved.kind else {
        return resolved;
    };
    if name == crate::ast::types::OPTION_TYPE_NAME && args.len() == 1 {
        let payload = canonical_declared_type(tc, &resolve_argument_type(tc, &args[0]));
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
/// The nullable half of `int?` rides on the type argument rather than on the
/// type inside it. Readers
/// that take the inner type alone — the element-drop path among them — then see
/// a bare `int` and treat the element as a value with nothing to release.
/// Folding the flag into the type is what makes `[int?]` and `[Option<int>]`
/// the single type they are meant to be. Arguments that are not types, such as
/// the size in `[T; N]`, are carried through untouched.
fn canonical_type_argument(
    tc: &crate::type_checker::TypeChecker,
    arg: &TypeArgument,
) -> TypeArgument {
    let TypeArgKind::Type(ty, is_nullable) = &arg.node else {
        return arg.clone();
    };
    let inner = canonical_declared_type(tc, ty);
    let canonical = if *is_nullable {
        Type::new(TypeKind::Option(Box::new(inner)), ty.span)
    } else {
        inner
    };
    rewritten(arg, TypeArgKind::Type(Box::new(canonical), false))
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
        release_consumed_binding(ctx, init_expr, *span)?;
    } else {
        start_optional_absent(ctx, local, *span);
    }
    ctx.bind_local_name(decl.name.clone(), local);
    Ok(())
}

/// Write the absent value into `local` when it is an optional declared with
/// no initializer: such a variable reads `None` until it is assigned, on
/// every path, and an optional held as its bytes has no other way to say so.
fn start_optional_absent(ctx: &mut LoweringContext, local: crate::mir::Local, span: Span) {
    let ty = ctx.body.local_decls[local.0].ty.clone();
    if !matches!(ty.kind, TypeKind::Option(_)) {
        return;
    }
    let absent = Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty,
        literal: Literal::None,
    }));
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(local), Rvalue::Use(absent)),
        span,
    });
}

/// End the storage of the binding `expr` was read from when the type checker
/// records that consuming it there hands its reference on
/// (`type_checker::use_after_move::ConsumedArguments`); its release lands
/// where its storage ends.
///
/// `var b = a` moves a value with a drop hook: the type checker consumes `a`,
/// so nothing reads it again. The copy into `b` shares the reference, and were
/// `a` to keep its own, `b.drop()` would not release the last one and the hook
/// would run only when `a`'s scope ends. Ending `a` here releases its reference
/// right after the copy; drop elaboration (`mir::drop_elaboration`) then
/// removes, or guards by a drop flag, the end of `a` its scope exit would
/// otherwise reach a second time. A call that keeps `a` but lends it consumes
/// it the same way, its callee holding a reference of its own; a callee that
/// owns the parameter takes `a`'s own reference by `move` instead
/// (`lowering::kept_parameters`).
pub(crate) fn release_consumed_binding(
    ctx: &mut LoweringContext,
    expr: &Expression,
    span: Span,
) -> Result<(), LoweringError> {
    if let Some(source) = consumed_read(ctx, expr)? {
        end_consumed_binding(ctx, source, span);
    }
    Ok(())
}

/// What became of a read the type checker records as consumed, by the id of
/// the expression consuming it ([`LoweringContext::consumed_reads`]).
pub(crate) enum ConsumedRead {
    /// The binding read, which the site consuming it has yet to end.
    Pending(crate::mir::Local),
    /// The call consuming it ended the binding where it handed it over
    /// (`lowering::kept_parameters`), so no later site ends it again.
    Ended,
}

/// The bindings a call's arguments consume, the receiver of a method call
/// among them, each once, with the id of the first argument naming it. Each
/// read is taken: the caller ends every binding returned.
pub(crate) fn consumed_argument_locals(
    ctx: &mut LoweringContext,
    func: &Expression,
    args: &[Expression],
) -> Result<Vec<(usize, crate::mir::Local)>, LoweringError> {
    let receiver = if let ExpressionKind::Member(receiver, _) = &func.node {
        Some(receiver.as_ref())
    } else {
        None
    };
    let mut consumed: Vec<(usize, crate::mir::Local)> = Vec::new();
    for argument in receiver.into_iter().chain(args.iter().map(argument_value)) {
        let Some(local) = consumed_read(ctx, argument)? else {
            continue;
        };
        if !consumed.iter().any(|&(_, found)| found == local) {
            consumed.push((argument.id, local));
        }
    }
    Ok(consumed)
}

/// The value an argument passes: a named argument's value, or the argument.
fn argument_value(arg: &Expression) -> &Expression {
    if let ExpressionKind::NamedArgument(_, value) = &arg.node {
        value
    } else {
        arg
    }
}

/// Whether `local` is a binding of the body's own rather than a parameter,
/// whose reference belongs to the caller.
pub(crate) fn is_own_binding(ctx: &LoweringContext, local: crate::mir::Local) -> bool {
    local.0 > ctx.body.arg_count
}

/// The binding the lowering of `expr` read, when the type checker records
/// that consuming `expr` hands its reference on; `None` for any other
/// expression, and for one whose call already ended the binding. Refused when the lowering of a consumed expression read no
/// binding, which would leave the binding released only where its scope ends.
pub(crate) fn consumed_read(
    ctx: &mut LoweringContext,
    expr: &Expression,
) -> Result<Option<crate::mir::Local>, LoweringError> {
    if !ctx.type_checker.consumed_arguments.consumes(expr) {
        return Ok(None);
    }
    match ctx.consumed_reads.remove(&expr.id) {
        Some(ConsumedRead::Pending(local)) => Ok(Some(local)),
        Some(ConsumedRead::Ended) => Ok(None),
        None => Err(LoweringError::internal(
            crate::diagnostics::DiagnosticCode::MirValidationFailed,
            "a consumed argument was lowered without reading the binding it names",
            expr.span,
        )),
    }
}

/// End `local`'s storage where a consuming site gave its value up, and
/// whether it did. A parameter's reference belongs to the caller, so
/// consuming a parameter ends nothing here.
pub(crate) fn end_consumed_binding(
    ctx: &mut LoweringContext,
    local: crate::mir::Local,
    span: Span,
) -> bool {
    if !is_own_binding(ctx, local) {
        return false;
    }
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::StorageDead(Place::new(local), crate::mir::EndCause::Consumed),
        span,
    });
    true
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
    let op_ty = super::helpers::operand_value_type(ctx, &op);
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
