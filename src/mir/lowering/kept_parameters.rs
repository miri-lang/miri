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

/// Records `kept` on the body's signature and binds, at the body's entry,
/// each parameter it owns to a local of its own holding the reference the
/// caller handed over, so every use of the parameter's name reads the local.
///
/// Refused when a GPU body owns a parameter, or an owned parameter has no
/// binding to move from: the type checker keeps no parameter of either.
pub(crate) fn record_and_bind_kept_parameters(
    ctx: &mut LoweringContext,
    params: &[Parameter],
    kept: &[bool],
    span: Span,
) -> Result<KeptParameterBindings, LoweringError> {
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
    for (param, _) in params.iter().zip(kept).filter(|(_, is_kept)| **is_kept) {
        let Some(&local) = ctx.variable_map.get(param.name.as_str()) else {
            let message = format!("the owned parameter `{}` has no binding", param.name);
            let span = param.typ.span;
            return Err(LoweringError::internal(
                DiagnosticCode::MirValidationFailed,
                message,
                span,
            ));
        };
        bind_owned_parameter(ctx, param, local);
    }
    Ok(KeptParameterBindings { is_scoped: true })
}

/// Binds `param`'s name to a new local the parameter `local` moves into.
///
/// The local is numbered past `arg_count`, so it is the binding a consuming
/// site ends (`variable::end_consumed_binding`) and a drop hook call releases
/// (`drop_hook_call::release_owned_local`), as a `let` would be.
fn bind_owned_parameter(ctx: &mut LoweringContext, param: &Parameter, local: Local) {
    let span = param.typ.span;
    let ty = ctx.body.local_decls[local.0].ty.clone();
    let owned = ctx.alloc_local(param.name.clone(), ty, span);
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::Assign(
            Place::new(owned),
            Rvalue::Use(Operand::Move(Place::new(local))),
        ),
        span,
    });
    ctx.bind_local_name(param.name.clone(), owned);
}

/// A direct call, with the parameters its callee owns.
pub(crate) struct KeptCall<'a> {
    /// The expression naming the callee.
    pub callee: &'a Expression,
    /// Which parameters the callee owns, in parameter order; empty when it
    /// owns none.
    pub kept: &'a [bool],
    /// The arguments as written.
    pub args: &'a [Expression],
    /// The first local the call's arguments were lowered into.
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
            callee,
            kept,
            args,
            arg_watermark,
            destination,
        })
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
        let span = call.callee.span;
        return Err(LoweringError::internal(
            DiagnosticCode::MirValidationFailed,
            message,
            span,
        ));
    }
    if arg_modes.len() < arg_ops.len() {
        arg_modes.resize(arg_ops.len(), ArgMode::Borrowed);
    }
    let consumed = variable::consumed_argument_locals(ctx, call.callee, call.args)?;
    for &(argument, _) in &consumed {
        ctx.consumed_reads.insert(argument, ConsumedRead::Ended);
    }
    let owned_positions = (call.kept.iter().enumerate()).filter(|(_, is_kept)| **is_kept);
    for (position, _) in owned_positions {
        arg_modes[position] = ArgMode::Owned;
        let Some(local) = whole_local_copied(&arg_ops[position]) else {
            continue;
        };
        let is_consumed_binding =
            consumed.iter().any(|&(_, read)| read == local) && variable::is_own_binding(ctx, local);
        if is_consumed_binding || is_call_temporary(ctx, call, local) {
            arg_ops[position] = Operand::Move(Place::new(local));
            handed.moved.push(local);
        }
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
