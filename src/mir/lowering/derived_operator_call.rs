// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering of an operator method — `equals`, `compare`, `concat`, `repeat` —
//! called on a value whose type answers it through the operator.
//!
//! A number, a `bool`, a struct and every other type that is not a class
//! answers the method an operator dispatches to by applying that operator: the
//! type checker admits the call (see `type_checker::derived_conformance`) and
//! this lowering builds it. `a.equals(b)` is `a == b`, `a.concat(b)` is
//! `a + b`, `a.repeat(n)` is `a * n`, and `a.compare(b)` is the three-way
//! comparison `<` reads. An integer answers the `Steppable` methods the same
//! way: `n.successor()` is `n + 1`, checked like any `+`, and `n.steppable()`
//! is `true`. A call on a class, or on a type declaring the method itself, is
//! an ordinary method call and is left to dispatch.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::implicit_methods::{operator_naming_method, ORDERING_METHOD_NAME};
use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::types::{Type, TypeKind, STEPPABLE_METHOD_NAME, SUCCESSOR_METHOD_NAME};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{BinOp, Constant, Operand, Place, Rvalue, Statement, StatementKind};

use super::derived_hash_call::try_lower_derived_hash_call;
use super::expression::binary_expr::{lower_binary_expr, lower_three_way_comparison};
use super::LoweringContext;

/// Lowers a call of a method the receiver's type answers without declaring
/// it — its derived `hash()`, or an operator method answered by the operator —
/// into `dest` when the call has one. Returns `None` for any other call.
pub(super) fn try_lower_derived_member_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    receiver: &Expression,
    method: &Expression,
    args: &[Expression],
    dest: Option<&Place>,
) -> Result<Option<Operand>, LoweringError> {
    if let Some(hash) = try_lower_derived_hash_call(ctx, span, receiver, method, args, dest)? {
        return Ok(Some(hash));
    }
    if let Some(step) = try_lower_derived_stepping_call(ctx, span, receiver, method, args, dest)? {
        return Ok(Some(step));
    }
    try_lower_derived_operator_call(ctx, span, call_expr_id, receiver, method, args, dest)
}

/// Lowers `receiver.method(argument)` when the receiver's type answers the
/// operator method through its operator, into `dest` when the call has one.
/// Returns `None` for any other call.
fn try_lower_derived_operator_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    receiver: &Expression,
    method: &Expression,
    args: &[Expression],
    dest: Option<&Place>,
) -> Result<Option<Operand>, LoweringError> {
    let ExpressionKind::Identifier(method_name, _) = &method.node else {
        return Ok(None);
    };
    let Some((op, operator_method)) = operator_naming_method(method_name) else {
        return Ok(None);
    };
    let [argument] = args else {
        return Ok(None);
    };
    let Some(receiver_ty) = ctx.recorded_type(receiver.id) else {
        return Ok(None);
    };
    if !ctx
        .type_checker
        .answers_operator_methods_by_operator(&receiver_ty, operator_method.name)
    {
        return Ok(None);
    }
    if operator_method.name == ORDERING_METHOD_NAME {
        return lower_three_way_comparison(ctx, receiver, argument, *span, dest.cloned()).map(Some);
    }
    // The operator node stands where the call stood, so it reads the call's
    // recorded type: the `bool` of `equals`, the receiver's own type for
    // `concat` and `repeat`.
    let applied = Expression {
        id: call_expr_id,
        node: ExpressionKind::Binary(Box::new(receiver.clone()), op, Box::new(argument.clone())),
        span: *span,
    };
    lower_binary_expr(ctx, &applied, dest.cloned()).map(Some)
}

/// Lowers `receiver.successor()` or `receiver.steppable()` on an integer,
/// which meets `Steppable` by value, into `dest` when the call has one.
/// Returns `None` for any other call.
///
/// The successor is the integer one above, added with the checked `+` every
/// integer sum uses, so the successor of a type's maximum stops the program
/// as an overflow. Every integer can start or end a range, so `steppable()`
/// is `true` once the receiver has been evaluated.
fn try_lower_derived_stepping_call(
    ctx: &mut LoweringContext,
    span: &Span,
    receiver: &Expression,
    method: &Expression,
    args: &[Expression],
    dest: Option<&Place>,
) -> Result<Option<Operand>, LoweringError> {
    let ExpressionKind::Identifier(method_name, _) = &method.node else {
        return Ok(None);
    };
    let is_successor = method_name == SUCCESSOR_METHOD_NAME;
    if (!is_successor && method_name != STEPPABLE_METHOD_NAME) || !args.is_empty() {
        return Ok(None);
    }
    let Some(receiver_ty) = ctx.recorded_type(receiver.id) else {
        return Ok(None);
    };
    if !ctx.type_checker.steps_by_machine(&receiver_ty) {
        return Ok(None);
    }
    let value = super::sequencing::lower_operand(ctx, receiver)?;
    let (rvalue, result_ty) = if is_successor {
        successor_of(value, receiver_ty, *span)?
    } else {
        always_steppable(*span)
    };
    let target = match dest {
        Some(place) => place.clone(),
        None => Place::new(ctx.push_temp(result_ty, *span)),
    };
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(target.clone(), rvalue),
        span: *span,
    });
    Ok(Some(Operand::Copy(target)))
}

/// `value + 1` at the integer type `ty`, and that type.
fn successor_of(value: Operand, ty: Type, span: Span) -> Result<(Rvalue, Type), LoweringError> {
    let one = IntegerLiteral::from_type_kind(&ty.kind, 1).ok_or_else(|| {
        LoweringError::internal(
            crate::diagnostics::DiagnosticCode::CgInternalCodegenError,
            format!("`{ty}` steps by machine but has no integer literal one"),
            span,
        )
    })?;
    let one = Operand::Constant(Box::new(Constant {
        span,
        ty: ty.clone(),
        literal: Literal::Integer(one),
    }));
    Ok((
        Rvalue::BinaryOp(BinOp::Add, Box::new(value), Box::new(one)),
        ty,
    ))
}

/// The `true` an integer's `steppable()` answers, and its type.
fn always_steppable(span: Span) -> (Rvalue, Type) {
    let boolean = Type::new(TypeKind::Boolean, span);
    let yes = Operand::Constant(Box::new(Constant {
        span,
        ty: boolean.clone(),
        literal: Literal::Boolean(true),
    }));
    (Rvalue::Use(yes), boolean)
}
