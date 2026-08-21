// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use crate::ast::statement::{BindingResidency as AstResidency, VariableDeclaration};
use crate::ast::types::{Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::body::{BindingResidency as MirResidency, DeviceHandleId};
use crate::mir::types::MirType;
use crate::mir::{
    Constant, Local, Operand, Place, Rvalue, StatementKind as MirStatementKind, StorageClass,
    Terminator, TerminatorKind,
};

use super::helpers::{coerce_rvalue, release_coerced_source};
use super::{lower_expression, resolve_type, LoweringContext};
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
///
/// Shared with `g.slice(range)` lowering, which fences the same way before
/// copying a sub-range of the device buffer back to host.
///
/// For gpu-resident scalars (e.g., a reduce result), creates a temporary
/// 1-element array wrapper, reads into it, then copies the scalar back.
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
    let Some(handle) = ctx.body.local_decls[src_local.0].device_handle else {
        return;
    };

    let src_ty = ctx.body.local_decls[src_local.0].ty.clone();
    let is_array = matches!(src_ty.kind, TypeKind::Array(_, _))
        || matches!(src_ty.kind, TypeKind::Custom(ref n, _)
            if crate::ast::types::BuiltinCollectionKind::from_name(n)
                == Some(crate::ast::types::BuiltinCollectionKind::Array));

    if is_array {
        // Standard array readback: pass the array directly
        let array_op = Operand::Copy(Place::new(src_local));
        emit_void_runtime_call(
            ctx,
            READBACK_FN,
            vec![handle_operand(handle, span), array_op],
            span,
        );
    } else {
        emit_scalar_readback(ctx, handle, src_local, &src_ty, span);
    }
}

/// Reads a gpu-resident scalar (e.g. a `gpu let` reduce result) back to host.
///
/// The readback runtime entry copies a device buffer into a host *array*, so a
/// lone scalar has no destination. Wrap it in a temporary 1-element
/// `Array<T, 1>`, read the device buffer into that array, then copy element 0
/// into the scalar local. The wrapper is dropped immediately afterwards.
fn emit_scalar_readback(
    ctx: &mut LoweringContext,
    handle: DeviceHandleId,
    src_local: Local,
    src_ty: &Type,
    span: Span,
) {
    use crate::ast::expression::ExpressionKind as AstExprKind;
    use crate::ast::types::BuiltinCollectionKind;
    use crate::mir::{AggregateKind, PlaceElem, Statement};

    let type_arg = |node| Expression { id: 0, node, span };
    let array_ty = Type::new(
        TypeKind::Custom(
            BuiltinCollectionKind::Array.name().to_string(),
            Some(vec![
                type_arg(AstExprKind::Type(Box::new(src_ty.clone()), false)),
                type_arg(AstExprKind::Literal(Literal::Integer(IntegerLiteral::I64(
                    1,
                )))),
            ]),
        ),
        span,
    );

    let temp_array = ctx.push_temp(array_ty, span);
    ctx.push_statement(Statement {
        kind: MirStatementKind::Assign(
            Place::new(temp_array),
            Rvalue::Aggregate(AggregateKind::Array, vec![zero_operand(src_ty, span)]),
        ),
        span,
    });

    emit_void_runtime_call(
        ctx,
        READBACK_FN,
        vec![
            handle_operand(handle, span),
            Operand::Copy(Place::new(temp_array)),
        ],
        span,
    );

    let zero_idx = ctx.push_temp(Type::new(TypeKind::Int, span), span);
    ctx.push_statement(Statement {
        kind: MirStatementKind::Assign(Place::new(zero_idx), Rvalue::Use(int_constant(0, span))),
        span,
    });
    let mut elem_place = Place::new(temp_array);
    elem_place.projection.push(PlaceElem::Index(zero_idx));

    ctx.push_statement(Statement {
        kind: MirStatementKind::Assign(
            Place::new(src_local),
            Rvalue::Use(Operand::Copy(elem_place)),
        ),
        span,
    });
    ctx.push_statement(Statement {
        kind: MirStatementKind::StorageDead(Place::new(temp_array)),
        span,
    });
}

/// A width-matched zero constant of `ty`. Only the type/width matters — the
/// readback overwrites the value — but a narrower/wider zero would lay the
/// temporary array element out differently from the device buffer and corrupt
/// the copy, so the float widths are matched exactly.
fn zero_operand(ty: &Type, span: Span) -> Operand {
    let literal = match ty.kind {
        TypeKind::F32 => Literal::Float(FloatLiteral::F32(0u32)),
        TypeKind::F64 | TypeKind::Float => Literal::Float(FloatLiteral::F64(0u64)),
        _ => Literal::Integer(IntegerLiteral::I64(0)),
    };
    Operand::Constant(Box::new(Constant {
        span,
        ty: ty.clone(),
        literal,
    }))
}

/// An `int`-typed integer constant operand.
fn int_constant(value: i64, span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Int, span),
        literal: Literal::Integer(IntegerLiteral::I64(value)),
    }))
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
fn canonical_declared_type(tc: &crate::type_checker::TypeChecker, ty: &Type) -> Type {
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
        let declared = resolve_type(ctx.type_checker, type_expr);
        let ty = ctx.resolve_self_in(&canonical_declared_type(ctx.type_checker, &declared));
        return Ok((ty, decl.initializer.as_deref(), None));
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
    if decl.residency == AstResidency::Host {
        emit_cross_residency_readback(ctx, decl.initializer.as_deref(), *span);
    }
    let (var_ty, init_expr_opt, pre_lowered_op) = resolve_decl_init(ctx, decl, span)?;
    let var_ty_kind = var_ty.kind.clone();
    // Allocate the local but defer binding its name: a shadowing initializer
    // (`let x = x + 1`) must resolve `x` to the outer binding, not the local we
    // are declaring. The name becomes resolvable only after the initializer is
    // lowered.
    let local = ctx.alloc_local(decl.name.clone(), var_ty, *span);

    apply_variable_residency(ctx, local, decl, span);

    if let Some(init_expr) = init_expr_opt {
        assign_variable_initializer(ctx, local, init_expr, pre_lowered_op, &var_ty_kind, span)?;
    }
    ctx.bind_local_name(decl.name.clone(), local);
    Ok(())
}

/// Apply shared-storage and host/gpu residency metadata to a freshly-declared
/// local, allocating a device handle (and emitting a buffer reset) for gpu vars.
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
    if ctx.body.local_decls[local.0].residency == MirResidency::Gpu {
        if let Some(handle) = gpu_move_source_handle(ctx, decl) {
            // `gpu let/var b = a` where `a` is a gpu-resident binding is a move:
            // `b` takes over `a`'s persistent device buffer (the type checker
            // has consumed `a`). Transfer the handle so `b`'s first launch
            // reuses the already-uploaded buffer, and skip the reset — releasing
            // here would drop the very buffer being transferred.
            ctx.body.local_decls[local.0].device_handle = Some(handle);
        } else {
            let handle = DeviceHandleId::fresh();
            ctx.body.local_decls[local.0].device_handle = Some(handle);
            emit_gpu_buffer_reset(ctx, handle, *span);
        }
    }
}

/// Device handle of a gpu-to-gpu move source: a bare identifier initializer
/// bound to a gpu-resident local with a live device handle. When target is
/// gpu-resident and source is a gpu binding, the moved binding inherits the
/// source buffer instead of allocating a fresh one.
fn gpu_move_source_handle(
    ctx: &LoweringContext,
    decl: &VariableDeclaration,
) -> Option<DeviceHandleId> {
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
    src_decl.device_handle
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
        coerce_rvalue(op.clone(), &op_ty, &target_ty)
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
