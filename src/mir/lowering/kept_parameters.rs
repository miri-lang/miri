// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Both sides of a call to a function that owns a resource parameter.
//!
//! The type checker decides which parameters a function owns: a resource its
//! escape summary keeps ([`crate::type_checker::kept_parameters`]). Such a
//! parameter is the callee's from the call on, so its drop hook runs where
//! the callee's new owner ends, not where the caller's binding would have.
//!
//! The callee's signature carries the decision
//! ([`crate::mir::Signature::kept_params`]), and its body binds each owned
//! parameter on entry to a local of its own, `let h = move _1`, in the entry
//! block, which nothing branches back to. Every use reads the local, so a
//! consuming end, a drop flag, the release where its scope ends and the
//! reverse order of declaration apply to it as to any local.
//!
//! Each direct call passes the argument at an owned position
//! [`ArgMode::Owned`]. A binding the call consumes, and a temporary the call
//! alone releases, are handed over by `move`, and their end after the call
//! releases nothing ([`EndCause::Moved`]). Anything else — a field, a
//! parameter the caller only borrows — is copied, and reference counting
//! funds the callee's reference with a retain.

use crate::ast::common::Parameter;
use crate::ast::expression::Expression;
use crate::ast::statement::RECEIVER_PARAM_NAME;
use crate::ast::types::TypeKind;
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::variable::{self, ConsumedRead};
use crate::mir::{
    ArgMode, EndCause, Local, Operand, Place, Rvalue, Statement as MirStatement,
    StatementKind as MirStatementKind,
};
use crate::type_checker::context::MethodInfo;
use crate::type_checker::TypeChecker;

/// The scope the owned locals of a body's kept parameters are bound in.
#[must_use]
pub(crate) struct KeptParameterBindings {
    is_scoped: bool,
}

impl KeptParameterBindings {
    /// Ends the scope of the owned locals once the body is lowered, releasing
    /// each one the body still holds when it falls off its end; a `return`
    /// released them already.
    pub(crate) fn close(self, ctx: &mut LoweringContext, span: Span) {
        if self.is_scoped {
            ctx.pop_scope(span);
        }
    }
}

/// The parameters a body binds by name, in the order a call passes its
/// operands: the receiver of an instance method, then each declared one.
pub(crate) struct NamedParameters<'a> {
    /// Where the receiver is declared, for an instance method; `None` for a
    /// body with no receiver.
    pub receiver: Option<Span>,
    /// The parameters a caller supplies.
    pub params: &'a [Parameter],
}

impl NamedParameters<'_> {
    fn len(&self) -> usize {
        usize::from(self.receiver.is_some()) + self.params.len()
    }

    /// Each parameter's name and where it is declared, in operand order.
    fn names(&self) -> impl Iterator<Item = (&str, Span)> {
        let receiver = (self.receiver).map(|span| (RECEIVER_PARAM_NAME, span));
        let params = (self.params.iter()).map(|param| (param.name.as_str(), param.typ.span));
        receiver.into_iter().chain(params)
    }
}

/// Records on the body's signature the parameters the function or method
/// the statement `declaration` declares owns, and binds, at the body's
/// entry, each one to a local of its own holding the reference the caller
/// handed over, so every use of the parameter's name reads the local. A
/// method owning its receiver binds `self` so; the signature's receiver
/// stays the parameter the caller passes. The analyses of a write through
/// the receiver read `Signature::receiver` and so do not see the owned
/// `self`; they only act on a receiver copied on write, and a class or an
/// enum, the only receivers the type checker keeps, never is one.
///
/// Refused when a GPU body owns a parameter, or an owned parameter has no
/// binding to move from: the type checker keeps no parameter of either.
pub(crate) fn record_and_bind_kept_parameters(
    ctx: &mut LoweringContext,
    declaration: usize,
    params: NamedParameters<'_>,
    span: Span,
) -> Result<KeptParameterBindings, LoweringError> {
    let kept = ctx.type_checker.kept_parameters_of_declaration(declaration);
    if !kept.contains(&true) {
        return Ok(KeptParameterBindings { is_scoped: false });
    }
    if ctx.body.is_gpu() || kept.len() != params.len() {
        let message = "a GPU body, or a parameter list the owned parameters do not match, \
                       owns a parameter";
        return Err(LoweringError::internal(
            DiagnosticCode::MirValidationFailed,
            message,
            span,
        ));
    }
    ctx.body.signature.kept_params = kept.to_vec();
    ctx.push_scope();
    for ((name, span), _) in params.names().zip(kept).filter(|(_, is_kept)| **is_kept) {
        let Some(&local) = ctx.variable_map.get(name) else {
            let message = format!("the owned parameter `{name}` has no binding");
            return Err(LoweringError::internal(
                DiagnosticCode::MirValidationFailed,
                message,
                span,
            ));
        };
        bind_owned_parameter(ctx, name, local, span);
    }
    Ok(KeptParameterBindings { is_scoped: true })
}

/// Binds the parameter `name` to a new local the parameter `local` moves
/// into.
///
/// The local is numbered past `arg_count`, so it is the binding a consuming
/// site ends (`variable::end_consumed_binding`) and a drop hook call releases
/// (`drop_hook_call::release_owned_local`), as a `let` would be.
fn bind_owned_parameter(ctx: &mut LoweringContext, name: &str, local: Local, span: Span) {
    let ty = ctx.body.local_decls[local.0].ty.clone();
    let owned = ctx.push_local(name.to_string(), ty, span);
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::Assign(
            Place::new(owned),
            Rvalue::Use(Operand::Move(Place::new(local))),
        ),
        span,
    });
}

/// A direct call, with the parameters its callee owns.
pub(crate) struct KeptCall<'a> {
    /// Where the call names its callee.
    pub span: Span,
    /// The receiver the call passes first, for a method call.
    pub receiver: Option<&'a Expression>,
    /// Which parameters the callee owns, in the order the call passes its
    /// operands, a method's receiver first; empty when it owns none.
    pub kept: &'a [bool],
    /// The arguments as written.
    pub args: &'a [Expression],
    /// The first local lowering the call's operands made: its receiver's,
    /// for a method call, else its arguments'.
    pub arg_watermark: usize,
    /// The local the call writes its result into.
    pub destination: Local,
}

impl<'a> KeptCall<'a> {
    /// The direct call to `func`, named by `callee`, whose declared type is
    /// the type checker's for the expression `signature_id`. A call through
    /// a value reaches no owner: only a call naming the function by its
    /// symbol passes an argument owned.
    ///
    /// Refused when the owned parameters do not match the parameters the
    /// callee declares, which would hand over the wrong argument.
    pub(crate) fn of(
        tc: &'a TypeChecker,
        callee: &'a Expression,
        signature_id: usize,
        func: &Operand,
        args: &'a [Expression],
        (arg_watermark, destination): (usize, Local),
    ) -> Result<Self, LoweringError> {
        let kept = match func {
            Operand::Function(_) => tc.kept_parameters_of_callee(callee.id),
            Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) => &[],
        };
        let declared = (tc.get_type(signature_id)).and_then(|ty| {
            let TypeKind::Function(function) = &ty.kind else {
                return None;
            };
            Some(function.params.len())
        });
        if !kept.is_empty() && declared != Some(kept.len()) {
            let message = "the parameters a callee owns do not match the parameters it declares";
            return Err(LoweringError::internal(
                DiagnosticCode::MirValidationFailed,
                message,
                callee.span,
            ));
        }
        Ok(KeptCall {
            span: callee.span,
            receiver: variable::member_receiver(callee),
            kept,
            args,
            arg_watermark,
            destination,
        })
    }

    /// This call, checked against the method `method` it calls by name:
    /// its owned parameters, in operand order, cover the receiver of an
    /// instance method and each parameter it declares.
    ///
    /// Refused when they do not, which would hand over the wrong argument.
    pub(crate) fn checked_against_method(self, method: &MethodInfo) -> Result<Self, LoweringError> {
        let operands = method.params.len() + usize::from(!method.is_static);
        if self.kept.is_empty() || self.kept.len() == operands {
            return Ok(self);
        }
        let message = "the parameters a method owns do not match the operands its calls pass";
        Err(LoweringError::internal(
            DiagnosticCode::MirValidationFailed,
            message,
            self.span,
        ))
    }

    /// A call the compiler makes where the program wrote none, to a callee
    /// owning no parameter: it lends every operand.
    pub(crate) fn lending(span: Span, (arg_watermark, destination): (usize, Local)) -> Self {
        KeptCall {
            span,
            receiver: None,
            kept: &[],
            args: &[],
            arg_watermark,
            destination,
        }
    }
}

/// The bindings and temporaries a call took over, which it ends after it
/// returns ([`end_handed_over`]).
#[derive(Default)]
pub(crate) struct HandedOver {
    /// Handed over by `move`: each ends releasing nothing.
    pub moved: Vec<Local>,
    /// Consumed, but passed by copy: each ends releasing its reference.
    pub consumed: Vec<Local>,
}

impl HandedOver {
    /// The locals no temporary release after the call may end: each one the
    /// call took over by `move`, and the call's `destination`.
    pub(crate) fn held_with(&self, destination: Local) -> Vec<Local> {
        let mut held = Vec::with_capacity(self.moved.len() + 1);
        held.extend_from_slice(&self.moved);
        held.push(destination);
        held
    }
}

/// Passes each argument at a position the callee owns [`ArgMode::Owned`],
/// by `move` when the caller gives up a reference of its own: a binding of
/// the caller's the call consumes, or a temporary only the call releases.
/// Returns what the call took over, each of which [`end_handed_over`] ends
/// after the call, and marks the call's consumed reads ended there.
pub(crate) fn hand_over_kept_arguments(
    ctx: &mut LoweringContext,
    call: &KeptCall<'_>,
    arg_ops: &mut [Operand],
    arg_modes: &mut Vec<ArgMode>,
) -> Result<HandedOver, LoweringError> {
    let mut handed = HandedOver::default();
    if !call.kept.contains(&true) {
        return Ok(handed);
    }
    if arg_ops.len() < call.kept.len() {
        let message = "a call passes fewer arguments than its callee owns parameters";
        return Err(LoweringError::internal(
            DiagnosticCode::MirValidationFailed,
            message,
            call.span,
        ));
    }
    if arg_modes.len() < arg_ops.len() {
        arg_modes.resize(arg_ops.len(), ArgMode::Borrowed);
    }
    let consumed = variable::consumed_operand_locals(ctx, call.receiver, call.args)?;
    for &(argument, _) in &consumed {
        ctx.consumed_reads.insert(argument, ConsumedRead::Ended);
    }
    let owned_positions = (call.kept.iter().enumerate()).filter(|(_, is_kept)| **is_kept);
    for (position, _) in owned_positions {
        arg_modes[position] = ArgMode::Owned;
        // TODO: a field read, an element read or an `out` parameter handed
        // to a keeper is copied and funded with a retain, so the value has
        // two holders until the call's new owner ends: reading a resource
        // out of a field is not yet a borrow that cannot be stored, and an
        // `out` parameter handed to a keeper is not yet refused.
        let Some(local) = whole_local_copied(&arg_ops[position]) else {
            continue;
        };
        let is_consumed_binding =
            consumed.iter().any(|&(_, read)| read == local) && variable::is_own_binding(ctx, local);
        if !is_consumed_binding && !is_call_temporary(ctx, call, local) {
            continue;
        }
        if handed.moved.contains(&local) {
            let message = "a call hands the same local over at two owned positions";
            return Err(LoweringError::internal(
                DiagnosticCode::MirValidationFailed,
                message,
                call.span,
            ));
        }
        arg_ops[position] = Operand::Move(Place::new(local));
        handed.moved.push(local);
    }
    handed.consumed = (consumed.into_iter())
        .map(|(_, local)| local)
        .filter(|local| !handed.moved.contains(local))
        .collect();
    Ok(handed)
}

/// The local `operand` copies whole, if it copies one.
fn whole_local_copied(operand: &Operand) -> Option<Local> {
    match operand {
        Operand::Copy(place) if place.projection.is_empty() => Some(place.local),
        Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) | Operand::Function(_) => None,
    }
}

/// Whether `local` is a counted temporary lowering the call's arguments made
/// that only the call releases.
fn is_call_temporary(ctx: &LoweringContext, call: &KeptCall<'_>, local: Local) -> bool {
    local != call.destination
        && ctx.is_released_by_expression(local, call.arg_watermark)
        && ctx.is_counted(&ctx.body.local_decls[local.0].ty.kind)
}

/// What follows a direct call once it returns, the continuation aside: every
/// temporary among the locals its operands read, `arg_locals`, that the call
/// did not take over released, and what it took over ended.
pub(crate) fn release_after_kept_call(
    ctx: &mut LoweringContext,
    call: &KeptCall<'_>,
    arg_locals: &[Local],
    handed: &HandedOver,
    span: Span,
) {
    let held = handed.held_with(call.destination);
    super::dispatch::emit_direct_call_drops(ctx, arg_locals, call.arg_watermark, &held, span);
    end_handed_over(ctx, handed, span);
}

/// Ends, after the call, what [`hand_over_kept_arguments`] handed it: each
/// local taken by `move` without a release, and each consumed binding passed
/// by copy releasing its own reference, as any consuming site does.
pub(crate) fn end_handed_over(ctx: &mut LoweringContext, handed: &HandedOver, span: Span) {
    for &local in &handed.moved {
        ctx.push_statement(MirStatement {
            kind: MirStatementKind::StorageDead(Place::new(local), EndCause::Moved),
            span,
        });
    }
    for &local in &handed.consumed {
        variable::end_consumed_binding(ctx, local, span);
    }
}

/// Refuses a call the compiler makes where the program wrote none — one that
/// lends every operand — to the method `method` of `owner` when that method
/// owns a parameter, which it would release though no caller handed it one.
///
/// The type checker keeps no parameter of a method the compiler calls so
/// ([`crate::ast::implicit_methods::is_called_without_a_written_call`]);
/// this holds each such call site to that decision.
pub(crate) fn refuse_lending_to_a_keeper(
    ctx: &LoweringContext,
    (owner, method): (&str, &str),
    span: Span,
) -> Result<(), LoweringError> {
    if !ctx
        .type_checker
        .kept_parameters_of_method(owner, method)
        .contains(&true)
    {
        return Ok(());
    }
    let message =
        format!("the compiler calls `{owner}.{method}`, which owns a parameter, lending it");
    Err(LoweringError::internal(
        DiagnosticCode::MirValidationFailed,
        message,
        span,
    ))
}
