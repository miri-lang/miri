// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering of `value.drop()`, a direct call of a resource's drop hook.
//!
//! The hook runs when a value's last reference is released, so calling it as an
//! ordinary method would run it once for the call and again at that release.
//! The call instead releases the value at the call site. A local's slot is then
//! cleared, so the scope-exit release of the now null slot does nothing; a call
//! result is released as its temporary, which nothing releases again. Either
//! way the hook runs exactly once.
//!
//! The type checker admits the call only on a value the scope owns — a local it
//! declared, or a call result — and consumes that local, so nothing reads the
//! cleared slot afterwards.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::Literal;
use crate::ast::statement::DROP_HOOK_NAME;
use crate::ast::types::{Type, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::{LoweringError, LoweringErrorKind};
use crate::error::syntax::Span;
use crate::mir::{Constant, Operand, Place, Rvalue, Statement, StatementKind};
use crate::type_checker::use_after_move::{BORROWED_DROP_HELP, BORROWED_DROP_MESSAGE};
use crate::type_checker::utils::runs_drop_hook;

use super::{lower_expression, LoweringContext};

/// Lowers `receiver.drop()` when it calls the receiver's drop hook. Returns
/// `None` for any other call, including an ordinary method named `drop`.
pub(super) fn try_lower_drop_hook_call(
    ctx: &mut LoweringContext,
    span: &Span,
    receiver: &Expression,
    method: &Expression,
    args: &[Expression],
) -> Result<Option<Operand>, LoweringError> {
    let Some(receiver_ty) = drop_hook_receiver_type(ctx, receiver, method, args) else {
        return Ok(None);
    };
    if matches!(receiver.node, ExpressionKind::Call(_, _)) {
        release_call_result(ctx, receiver, *span)?;
    } else {
        release_owned_local(ctx, receiver, receiver_ty, *span)?;
    }
    Ok(Some(void_operand(*span)))
}

/// Releases a call result right away, through the same temporary release any
/// consumer of a call result performs, instead of at the end of the statement.
fn release_call_result(
    ctx: &mut LoweringContext,
    receiver: &Expression,
    span: Span,
) -> Result<(), LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let temp = match lower_expression(ctx, receiver, None)? {
        Operand::Copy(place) | Operand::Move(place) if place.projection.is_empty() => place.local,
        Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) => {
            return Err(borrowed_receiver_error(receiver.span));
        }
    };
    ctx.emit_temp_drop(temp, watermark, span);
    Ok(())
}

/// Releases a local the scope declared and clears its slot, so the release at
/// the end of its scope finds nothing to release.
///
/// TODO: the release runs the hook only when this local holds the last
/// reference. After `var g = h` the moved-from `h` still holds a retained copy
/// until its own scope ends, so `g.drop()` runs the hook there instead of at the
/// call: still once, but later than written. A resource move has to leave its
/// source holding nothing for `drop()` to release on the spot.
///
/// A parameter's reference belongs to the caller, and a projection's to its
/// container; releasing either here would release it twice. The type checker
/// refuses those receivers, so reaching one is a broken invariant, reported
/// rather than lowered into a double release.
fn release_owned_local(
    ctx: &mut LoweringContext,
    receiver: &Expression,
    receiver_ty: Type,
    span: Span,
) -> Result<(), LoweringError> {
    let ExpressionKind::Identifier(name, _) = &receiver.node else {
        return Err(borrowed_receiver_error(receiver.span));
    };
    let slot = ctx
        .variable_map
        .get(name.as_str())
        .copied()
        .filter(|local| local.0 > ctx.body.arg_count)
        .map(Place::new)
        .ok_or_else(|| borrowed_receiver_error(receiver.span))?;
    ctx.push_statement(Statement {
        kind: StatementKind::DecRef(slot.clone()),
        span,
    });
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(slot, Rvalue::Use(null_of(receiver_ty, span))),
        span,
    });
    Ok(())
}

/// The receiver's type when the call names the drop hook that type runs.
fn drop_hook_receiver_type(
    ctx: &LoweringContext,
    receiver: &Expression,
    method: &Expression,
    args: &[Expression],
) -> Option<Type> {
    let ExpressionKind::Identifier(method_name, _) = &method.node else {
        return None;
    };
    if method_name != DROP_HOOK_NAME || !args.is_empty() {
        return None;
    }
    let receiver_ty = ctx.recorded_type(receiver.id)?;
    runs_drop_hook(&receiver_ty.kind, ctx.type_checker.type_definitions()).then_some(receiver_ty)
}

fn borrowed_receiver_error(span: Span) -> LoweringError {
    LoweringError {
        kind: LoweringErrorKind::Coded {
            code: DiagnosticCode::OwnDropOfBorrowedValue,
            message: BORROWED_DROP_MESSAGE.to_string(),
            help: Some(BORROWED_DROP_HELP.to_string()),
        },
        span,
    }
}

/// A null reference of `ty`, which a release skips.
fn null_of(ty: Type, span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty,
        literal: Literal::None,
    }))
}

fn void_operand(span: Span) -> Operand {
    null_of(Type::new(TypeKind::Void, span), span)
}
