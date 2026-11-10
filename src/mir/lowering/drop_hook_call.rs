// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering of `value.drop()`, a direct call of a resource's drop hook.
//!
//! The hook runs when a value's last reference is released, so calling it as an
//! ordinary method would run it once for the call and again at that release.
//! The call instead releases the value at the call site. A local's storage is
//! ended there, and drop elaboration (`mir::drop_elaboration`) removes, or
//! guards by a drop flag, the end its scope exit would reach again; a call
//! result is released as its temporary, which nothing releases again. Either
//! way the hook runs exactly once.
//!
//! The type checker admits the call only on a value the scope owns — a local it
//! declared, or a call result — and consumes that local, so nothing reads it
//! afterwards.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::Literal;
use crate::ast::statement::DROP_HOOK_NAME;
use crate::ast::types::{Type, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{Constant, Operand, Place, Statement, StatementKind};
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
    if !calls_drop_hook(ctx, receiver, method, args) {
        return Ok(None);
    }
    if matches!(receiver.node, ExpressionKind::Call(_, _)) {
        release_call_result(ctx, receiver, *span)?;
    } else {
        release_owned_local(ctx, receiver, *span)?;
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
        Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) | Operand::Function(_) => {
            return Err(borrowed_receiver_error(receiver.span));
        }
    };
    ctx.emit_temp_drop(temp, watermark, span);
    Ok(())
}

/// Releases a local the scope declared by ending its storage here. A local a
/// resource was moved into holds its only reference, since the move released
/// its source's, so the hook runs here.
///
/// A parameter's reference belongs to the caller, and a projection's to its
/// container; releasing either here would release it twice. The type checker
/// refuses those receivers, so reaching one is a broken invariant, reported
/// rather than lowered into a double release.
fn release_owned_local(
    ctx: &mut LoweringContext,
    receiver: &Expression,
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
        kind: StatementKind::StorageDead(slot, crate::mir::EndCause::Consumed),
        span,
    });
    Ok(())
}

/// Whether the call names the drop hook its receiver's type runs.
fn calls_drop_hook(
    ctx: &LoweringContext,
    receiver: &Expression,
    method: &Expression,
    args: &[Expression],
) -> bool {
    let ExpressionKind::Identifier(method_name, _) = &method.node else {
        return false;
    };
    method_name == DROP_HOOK_NAME
        && args.is_empty()
        && ctx.recorded_type(receiver.id).is_some_and(|receiver_ty| {
            runs_drop_hook(&receiver_ty.kind, ctx.type_checker.type_definitions())
        })
}

fn borrowed_receiver_error(span: Span) -> LoweringError {
    LoweringError::coded(
        DiagnosticCode::OwnDropOfBorrowedValue,
        BORROWED_DROP_MESSAGE.to_string(),
        span,
        Some(BORROWED_DROP_HELP.to_string()),
    )
}

/// The unit value the call evaluates to.
fn void_operand(span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Void, span),
        literal: Literal::None,
    }))
}
