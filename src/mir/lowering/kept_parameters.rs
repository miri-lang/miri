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
//! A call through a function value or a closure follows the convention of
//! the value's type ([`value_call_convention`]); every body a value reaches
//! — a lambda, a nested function, the forwarding thunk of a function named
//! as a value — owns exactly those positions
//! ([`record_and_bind_value_parameters`]), and a thunk hands its target what
//! the target keeps ([`hand_over_owned_locals`]).
//!
//! Each direct call passes the argument at an owned position
//! [`ArgMode::Owned`]. A binding the call consumes, and a temporary the call
//! alone releases, are handed over by `move`, and their end after the call
//! releases nothing ([`EndCause::Moved`]). A payload bound out of a
//! scrutinee the scope owns is handed over with it: the scrutinee ends at the
//! call. Anything else — a plain field, a parameter the caller only borrows
//! — is copied, and reference counting funds the callee's reference with a
//! retain; a resource read out of any other place never reaches an owned
//! position ([`check_funded_argument`]).

use crate::ast::common::Parameter;
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::Literal;
use crate::ast::statement::RECEIVER_PARAM_NAME;
use crate::ast::types::{FunctionTypeData, Type, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::variable::{self, ConsumedRead};
use crate::mir::option_layout::{is_nullable_pointer_payload, is_payload_enum, taken_payload_hole};
use crate::mir::place::PlaceElem;
use crate::mir::{
    ArgMode, Body, CallFlags, Constant, EndCause, Local, Operand, Place, Rvalue,
    Statement as MirStatement, StatementKind as MirStatementKind, Terminator, TerminatorKind,
    RETURN_LOCAL,
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
    bind_kept_parameters(ctx, kept, params, span)
}

/// [`record_and_bind_kept_parameters`] for the copy of the method the
/// statement `declaration` declares being lowered: it owns what it keeps at
/// the types it is compiled at, each parameter of the body read through the
/// instantiation it is lowered for
/// ([`crate::type_checker::TypeChecker::kept_parameters_of_method_declaration`]),
/// the types every call naming the copy reads it at.
pub(crate) fn record_and_bind_kept_method_parameters(
    ctx: &mut LoweringContext,
    declaration: usize,
    params: NamedParameters<'_>,
    span: Span,
) -> Result<KeptParameterBindings, LoweringError> {
    let operand_types: Vec<Type> = (ctx.body.local_decls.iter().skip(1).take(params.len()))
        .map(|decl| compiled_operand_type(ctx, &decl.ty))
        .collect();
    let kept =
        (ctx.type_checker).kept_parameters_of_method_declaration(declaration, &operand_types);
    bind_kept_parameters(ctx, &kept, params, span)
}

/// The type a parameter declared at `ty` holds in the copy being lowered, in
/// the canonical form a call passing it reads it at
/// (`method_dispatch::method_argument_types`). The parameter is declared at
/// the copy's instantiation already (`lower_class_method_body`), so `ty` is
/// not read through the substitution again.
fn compiled_operand_type(ctx: &LoweringContext, ty: &Type) -> Type {
    variable::canonical_declared_type(ctx.type_checker, ty)
}

/// Records `kept`, in operand order, on the body's signature and binds each
/// parameter of `params` it marks to a local of its own.
fn bind_kept_parameters(
    ctx: &mut LoweringContext,
    kept: &[bool],
    params: NamedParameters<'_>,
    span: Span,
) -> Result<KeptParameterBindings, LoweringError> {
    if !kept.contains(&true) {
        return Ok(KeptParameterBindings { is_scoped: false });
    }
    if kept.len() != params.len() {
        return Err(mismatched_owned_parameters(span));
    }
    let owned = (params.names().zip(kept)).map(|((name, span), &is_kept)| (name, span, is_kept));
    let (bindings, _) = bind_owned_parameters(ctx, kept.to_vec(), owned, span)?;
    Ok(bindings)
}

/// Records on the signature of a body reached only through a function
/// value — a lambda, a nested function, the forwarding body of a function
/// named as a value — the parameters it owns, `convention` in the order
/// the value's calls pass their arguments
/// ([`crate::type_checker::kept_parameters::function_value_convention`]),
/// and binds each one at the body's entry to a local of its own. The body's
/// first parameter is its environment, which it never owns.
///
/// Returns, for each of `params`, the local an owned one was bound to.
pub(crate) fn record_and_bind_value_parameters<'p>(
    ctx: &mut LoweringContext,
    convention: &[bool],
    params: impl ExactSizeIterator<Item = (&'p str, Span)>,
    span: Span,
) -> Result<(KeptParameterBindings, Vec<Option<Local>>), LoweringError> {
    if params.len() != convention.len() {
        return Err(mismatched_owned_parameters(span));
    }
    let kept_params = std::iter::once(false)
        .chain(convention.iter().copied())
        .collect();
    let owned = (params.zip(convention)).map(|((name, span), &is_kept)| (name, span, is_kept));
    bind_owned_parameters(ctx, kept_params, owned, span)
}

/// Records on the signature of a forwarding body the parameters it owns,
/// `kept_params`, one per parameter in the order a call passes them, and
/// binds each owned one of `params` at the body's entry to a local of its
/// own. Returns, for each parameter, the local an owned one was bound to.
pub(crate) fn record_and_bind_owned_parameters<'p>(
    ctx: &mut LoweringContext,
    kept_params: Vec<bool>,
    params: impl ExactSizeIterator<Item = (&'p str, Span)>,
    span: Span,
) -> Result<(KeptParameterBindings, Vec<Option<Local>>), LoweringError> {
    if params.len() != kept_params.len() {
        return Err(mismatched_owned_parameters(span));
    }
    let owned =
        (params.zip(kept_params.clone())).map(|((name, span), is_kept)| (name, span, is_kept));
    bind_owned_parameters(ctx, kept_params, owned, span)
}

fn mismatched_owned_parameters(span: Span) -> LoweringError {
    LoweringError::internal(
        DiagnosticCode::MirValidationFailed,
        "a parameter list the owned parameters do not match owns a parameter",
        span,
    )
}

/// Records `kept_params` on the body's signature and binds each parameter
/// of `params` marked owned to a local of its own, returning for each one
/// the local it was bound to. Nothing is recorded or bound when no
/// parameter is owned.
fn bind_owned_parameters<'p>(
    ctx: &mut LoweringContext,
    kept_params: Vec<bool>,
    params: impl Iterator<Item = (&'p str, Span, bool)>,
    span: Span,
) -> Result<(KeptParameterBindings, Vec<Option<Local>>), LoweringError> {
    if !kept_params.contains(&true) {
        let unbound = params.map(|_| None).collect();
        return Ok((KeptParameterBindings { is_scoped: false }, unbound));
    }
    if ctx.body.is_gpu() {
        let message = "a GPU body owns a parameter";
        return Err(LoweringError::internal(
            DiagnosticCode::MirValidationFailed,
            message,
            span,
        ));
    }
    ctx.body.signature.kept_params = kept_params;
    ctx.push_scope();
    let mut bound = Vec::new();
    for (name, span, is_kept) in params {
        if !is_kept {
            bound.push(None);
            continue;
        }
        let Some(&local) = ctx.variable_map.get(name) else {
            let message = format!("the owned parameter `{name}` has no binding");
            return Err(LoweringError::internal(
                DiagnosticCode::MirValidationFailed,
                message,
                span,
            ));
        };
        bound.push(Some(bind_owned_parameter(ctx, name, local, span)));
    }
    Ok((KeptParameterBindings { is_scoped: true }, bound))
}

/// Binds the parameter `name` to a new local the parameter `local` moves
/// into.
///
/// The local is numbered past `arg_count`, so it is the binding a consuming
/// site ends (`variable::end_consumed_binding`) and a drop hook call releases
/// (`drop_hook_call::release_owned_local`), as a `let` would be.
fn bind_owned_parameter(ctx: &mut LoweringContext, name: &str, local: Local, span: Span) -> Local {
    let ty = ctx.body.local_decls[local.0].ty.clone();
    let owned = ctx.push_local(name.to_string(), ty, span);
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::Assign(
            Place::new(owned),
            Rvalue::Use(Operand::Move(Place::new(local))),
        ),
        span,
    });
    owned
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
    /// The call to `func`, named by `callee`, whose declared type is the type
    /// checker's for the expression `signature_id`. A call naming its
    /// function owns the parameters that function keeps; a call through a
    /// value owns what the value's convention, `value_convention`, marks
    /// ([`value_call_convention`]).
    ///
    /// Refused when the owned parameters do not match the parameters the
    /// callee declares, which would hand over the wrong argument.
    pub(crate) fn of(
        tc: &'a TypeChecker,
        (callee, signature_id): (&'a Expression, usize),
        func: &Operand,
        value_convention: Option<&'a [bool]>,
        args: &'a [Expression],
        (arg_watermark, destination): (usize, Local),
    ) -> Result<Self, LoweringError> {
        if let Some(kept) = value_convention {
            return Ok(KeptCall {
                span: callee.span,
                receiver: None,
                kept,
                args,
                arg_watermark,
                destination,
            });
        }
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
}

/// The positions a call through the value `callee` evaluates to, `func`,
/// owns ([`value_convention_in`]), read at the substitution being lowered,
/// so every instantiation of a generic body agrees with the bodies its
/// values reach; empty when the type checker recorded no function type for
/// `callee`. `None` for a call naming its function, which follows that
/// function's own convention.
pub(crate) fn value_call_convention(
    ctx: &LoweringContext,
    callee: &Expression,
    func: &Operand,
) -> Option<Vec<bool>> {
    match func {
        Operand::Copy(_) | Operand::Move(_) => {}
        Operand::Function(_) | Operand::Constant(_) => return None,
    }
    let Some(ty) = ctx.recorded_type(callee.id) else {
        return Some(Vec::new());
    };
    let TypeKind::Function(function) = &ty.kind else {
        return Some(Vec::new());
    };
    Some(value_convention_in(ctx, function))
}

/// The positions every call through a value of type `function` owns in the
/// body being lowered
/// ([`crate::type_checker::kept_parameters::function_value_convention`]); in
/// a GPU body none, since device code holds no counts.
pub(crate) fn value_convention_in(ctx: &LoweringContext, function: &FunctionTypeData) -> Vec<bool> {
    if ctx.body.is_gpu() {
        return vec![false; function.params.len()];
    }
    ctx.type_checker.function_value_convention(function)
}

/// The positions a call through the vtable slot of `selector` numbered under
/// `root` hands over: those the slot owns
/// ([`crate::type_checker::slot_conventions::SlotConventions`]), at the types
/// the call passes its operands at, `operand_types`, in operand order with
/// the receiver first. Empty when the slot owns none.
pub(crate) fn slot_call_convention<'t>(
    ctx: &LoweringContext,
    (root, selector): (&str, &str),
    operand_types: impl Iterator<Item = &'t Type>,
) -> Vec<bool> {
    slot_convention_at(
        (ctx.type_checker, &ctx.body),
        (root, selector),
        operand_types,
    )
}

/// The positions the vtable slot of `selector` numbered under `root` owns,
/// read at `operand_types`, one per position in operand order: a position
/// whose operand is no counted value — a generic trait's parameter at a
/// scalar type — is lent, on both sides of the call alike. Empty when it owns
/// none there, and in a GPU body, which holds no counts. `body` is the body
/// the types are read in: the caller's, or the body the slot is filled with.
pub(crate) fn slot_convention_at<'t>(
    (type_checker, body): (&TypeChecker, &Body),
    (root, selector): (&str, &str),
    operand_types: impl Iterator<Item = &'t Type>,
) -> Vec<bool> {
    let convention = type_checker.slot_convention(root, selector);
    if body.is_gpu() || !convention.contains(&true) {
        return Vec::new();
    }
    let owned: Vec<bool> = (convention.iter().zip(operand_types))
        .map(|(&is_owned, ty)| is_owned && body.is_counted(&ty.kind))
        .collect();
    if owned.contains(&true) {
        owned
    } else {
        Vec::new()
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
    if arg_modes.len() < arg_ops.len() {
        arg_modes.resize(arg_ops.len(), ArgMode::Borrowed);
    }
    let consumed = variable::consumed_operand_locals(ctx, call.receiver, call.args)?;
    for &(argument, _) in &consumed {
        ctx.consumed_reads.insert(argument, ConsumedRead::Ended);
    }
    let operands = funded_operands(ctx, call);
    let taken = |position, arg: &Operand| {
        let local = match lent_local(ctx, call, position, arg) {
            Some(lent) => Some(funded_copy(ctx, lent, call.span)),
            None => handed_over_local(ctx, call, &consumed, arg),
        };
        if let (None, Some(operands)) = (local, &operands) {
            check_funded_argument(ctx, call.span, operands.get(position).copied(), &consumed)?;
        }
        Ok(local)
    };
    handed.moved = pass_owned(call.kept, (arg_ops, arg_modes), call.span, taken)?;
    let payloads = hand_over_matched_payloads(ctx, call, arg_ops, &mut handed.moved);
    let ended: Vec<usize> = payloads.iter().map(|&(scrutinee, _)| scrutinee).collect();
    end_matched_scrutinees(ctx, call, (&consumed, &handed.moved), &ended);
    let is_ended_payload = |local: &Local| payloads.iter().any(|&(_, payload)| payload == *local);
    handed.consumed = (consumed.into_iter())
        .map(|(_, local)| local)
        .filter(|local| !handed.moved.contains(local) && !is_ended_payload(local))
        .collect();
    Ok(handed)
}

/// Hands the call, in place of each payload binding it takes by `move` that
/// a `Some(..)` or enum variant pattern bound out of a matched subject, the
/// payload's one reference, when the subject is the only local the match
/// holds its scrutinee in and the type checker records the hand-over as
/// ending that scrutinee ([`TypeChecker::ended_scrutinee`]). The payload
/// binding, and every binding of an inner match it was reached through,
/// then only reads what the subject holds, so none takes a reference of its
/// own, and each ends before the call.
///
/// An optional held as its payload's own pointer hands over its own
/// reference, `move o.0`, and ends, moved, after the call. A payload held in
/// a box the optional owns, or an optional an enum's payload field holds, is
/// taken out of it and its hole emptied ([`take_into`]); the subject ends
/// there, releasing the block. On a path
/// of the arm that hands nothing over, the bindings end with their scopes
/// and the optional's drop flag releases the payload where the optional
/// ends, so the payload has one holder on every path.
///
/// Rewrites `arg_ops` and `moved`, and returns each scrutinee it ended with
/// the payload binding it ended before the call.
fn hand_over_matched_payloads(
    ctx: &mut LoweringContext,
    call: &KeptCall<'_>,
    arg_ops: &mut [Operand],
    moved: &mut Vec<Local>,
) -> Vec<(usize, Local)> {
    let mut ended = Vec::new();
    let operands =
        (call.receiver.into_iter()).chain(call.args.iter().map(variable::argument_value));
    for (position, operand) in operands.enumerate() {
        let Some(scrutinee) = ctx.type_checker.ended_scrutinee(operand) else {
            continue;
        };
        let Some(payload) = arg_ops.get(position).and_then(moved_whole_local) else {
            continue;
        };
        let Some((handed_op, handed)) = matched_payload_operand(ctx, scrutinee, payload, call.span)
        else {
            continue;
        };
        arg_ops[position] = handed_op;
        moved.retain(|&local| local != payload);
        moved.push(handed);
        ended.push((scrutinee, payload));
    }
    ended
}

/// Hands a runtime entry point storing the value `value` lowered to, `op`,
/// the payload's one reference in place of the payload binding it reads,
/// when a `Some(..)` pattern bound it out of a matched optional the store
/// ends ([`TypeChecker::ended_scrutinee`]), as a keeper call is handed it
/// ([`hand_over_matched_payloads`]). A store passes the element
/// [`ArgMode::Donated`], which RC insertion funds only for a copy, so the
/// container keeps the optional's own reference rather than a retained copy
/// beside it. Answers the local the store takes over, which
/// [`end_handed_over`] ends, moved, after the call; `None`, rewriting
/// nothing, when `op` is not the binding's own read (one this copy of the
/// body does not consume), and the store funds a copy as before. A value
/// headed for an optional slot reaches here unwrapped: its `Some(..)` is
/// built around what this hands over (`StoredValue::hand_over`).
pub(crate) fn hand_over_stored_payload(
    ctx: &mut LoweringContext,
    value: &Expression,
    op: &mut Operand,
    span: Span,
) -> Option<Local> {
    let value = variable::argument_value(value);
    let scrutinee = ctx.type_checker.ended_scrutinee(value)?;
    let payload = whole_local_copied(op)?;
    let is_pending_read = matches!(
        ctx.consumed_reads.get(&value.id),
        Some(ConsumedRead::Pending(read)) if *read == payload
    );
    if !is_pending_read {
        return None;
    }
    let (handed_op, handed) = matched_payload_operand(ctx, scrutinee, payload, span)?;
    ctx.consumed_reads.insert(value.id, ConsumedRead::Ended);
    *op = handed_op;
    Some(handed)
}

/// Ends, moved, after a store returns, each local it took over in place of
/// a matched payload binding ([`hand_over_stored_payload`]).
pub(crate) fn end_stored_payloads(
    ctx: &mut LoweringContext,
    handed: impl IntoIterator<Item = Option<Local>>,
    span: Span,
) {
    let handed = HandedOver {
        moved: handed.into_iter().flatten().collect(),
        consumed: Vec::new(),
    };
    end_handed_over(ctx, &handed, span);
}

/// The operand handing a call the one reference the payload binding
/// `payload`, matched out of `scrutinee`, reads, with the local the call
/// takes over; the binding, and every binding it was reached through, ends
/// before the call. An optional held as its payload's own pointer hands over
/// its own reference, `move o.0`; a payload held in a box the optional owns,
/// or an optional an enum's payload field holds, is taken out of it
/// ([`take_into`]). A payload reached past an enum's field takes the field
/// out first, into a temporary of its own ([`enum_field_take`]), and is
/// then handed over out of that as out of any optional. `None`, emitting
/// nothing, when no subject the match holds alone holds the payload.
fn matched_payload_operand(
    ctx: &mut LoweringContext,
    scrutinee: usize,
    payload: Local,
    span: Span,
) -> Option<(Operand, Local)> {
    let mut path = payload_path(ctx, scrutinee, payload)?;
    let mut bindings = vec![payload];
    bindings.extend_from_slice(&path.through);
    let field = enum_field_take(ctx, &mut path, span);
    let matched = matched_payload(ctx, &path, payload)?;
    end_consumed(ctx, &bindings, span);
    if let Some((take, field)) = field {
        take_into(ctx, &take, field, span);
    }
    Some(match matched {
        MatchedPayload::OwnReference(optional) => {
            (Operand::Move(optional_payload(optional)), optional)
        }
        MatchedPayload::Boxed(take) => {
            let taken = ctx.push_temp(ctx.body.local_decls[payload.0].ty.clone(), span);
            take_into(ctx, &take, taken, span);
            (Operand::Move(Place::new(taken)), taken)
        }
    })
}

/// Where the payload a keeper is handed sits in the subject it was matched
/// out of.
enum MatchedPayload {
    /// In an optional held as its payload's own pointer, whose reference is
    /// the payload's.
    OwnReference(Local),
    /// In a block the matched subject holds: a box an optional owns, or an
    /// enum's.
    Boxed(BoxedPayload),
}

/// A payload held in a block the matched subject `subject` holds: a box of
/// an optional's, or an enum's, in one of its payload fields.
struct BoxedPayload {
    subject: Local,
    /// The payload, as a place under `subject`.
    place: Place,
    /// The optional a take of `place` empties ([`taken_payload_hole`]).
    hole: Place,
}

/// The way from a payload binding to the subject a match holds alone.
struct PayloadPath {
    subject: Local,
    /// The payload, as a place under `subject`.
    place: Place,
    /// The payload bindings of the matches between `subject` and the
    /// payload, each the subject of the next, outermost last.
    through: Vec<Local>,
}

/// The way the payload `payload` was bound out of the subject the match on
/// `scrutinee` holds as its only local: walking from its binding through
/// each payload a `Some(..)` or enum variant pattern bound it out of.
/// `None` when no such subject holds it.
fn payload_path(ctx: &LoweringContext, scrutinee: usize, payload: Local) -> Option<PayloadPath> {
    let holders = ctx.matched_scrutinees.get(&scrutinee)?;
    let mut place = ctx.bound_payloads.get(&payload)?.clone();
    let mut through = Vec::new();
    while holders.as_slice() != [place.local] {
        if through.len() == ctx.bound_payloads.len() {
            return None;
        }
        through.push(place.local);
        let outer = ctx.bound_payloads.get(&place.local)?;
        place
            .projection
            .splice(0..0, outer.projection.iter().cloned());
        place.local = outer.local;
    }
    Some(PayloadPath {
        subject: place.local,
        place,
        through,
    })
}

/// The take of the optional an enum's payload field holds, when `path`
/// reaches its payload past that field: a place past an enum's field names
/// no variant, so nothing could read it. The field is taken whole into a
/// temporary of the type of the binding bound out of it, and `path` is
/// rewritten to start there. Answers the take and the temporary; `None`,
/// rewriting nothing, when `path` starts at no enum, or ends at its field.
fn enum_field_take(
    ctx: &mut LoweringContext,
    path: &mut PayloadPath,
    span: Span,
) -> Option<(BoxedPayload, Local)> {
    let subject = &ctx.body.local_decls[path.subject.0].ty.kind;
    if !is_payload_enum(subject, ctx.body.layout_scope()) {
        return None;
    }
    let (field, rest) = path.place.projection.split_first()?;
    if rest.is_empty() {
        return None;
    }
    let bound = *path.through.last()?;
    let ty = ctx.body.local_decls[bound.0].ty.clone();
    let place = Place {
        local: path.subject,
        projection: vec![field.clone()],
    };
    let hole = taken_payload_hole(&place, &ty.kind, &ctx.body)?;
    let taken = ctx.push_temp(ty, span);
    let take = BoxedPayload {
        subject: path.subject,
        place,
        hole,
    };
    *path = PayloadPath {
        subject: taken,
        place: Place {
            local: taken,
            projection: rest.to_vec(),
        },
        through: path.through[..path.through.len() - 1].to_vec(),
    };
    Some((take, taken))
}

/// Where the payload `payload` reached along `path` sits; `None` when it
/// cannot leave its subject without a release freeing the block but not its
/// contents.
fn matched_payload(
    ctx: &LoweringContext,
    path: &PayloadPath,
    payload: Local,
) -> Option<MatchedPayload> {
    if path.through.is_empty() && is_nullable_pointer_payload(&path.place, &ctx.body) {
        return Some(MatchedPayload::OwnReference(path.subject));
    }
    let moved = &ctx.body.local_decls[payload.0].ty.kind;
    let hole = taken_payload_hole(&path.place, moved, &ctx.body)?;
    Some(MatchedPayload::Boxed(BoxedPayload {
        subject: path.subject,
        place: path.place.clone(),
        hole,
    }))
}

/// Takes the payload `take` names out of its block before the call, every
/// binding reading it ended already: the payload moves into `taken`, and
/// `None` fills the hole it leaves in the statement right after, so the
/// block holds nothing the call was handed. The matched subject is consumed
/// there, releasing the block.
fn take_into(ctx: &mut LoweringContext, take: &BoxedPayload, taken: Local, span: Span) {
    let absent = Operand::Constant(Box::new(Constant {
        span,
        ty: taken_hole_type(ctx, take, taken),
        literal: Literal::None,
    }));
    for (place, rvalue) in [
        (
            Place::new(taken),
            Rvalue::Use(Operand::Move(take.place.clone())),
        ),
        (take.hole.clone(), Rvalue::Use(absent)),
    ] {
        ctx.push_statement(MirStatement {
            kind: MirStatementKind::Assign(place, rvalue),
            span,
        });
    }
    end_consumed(ctx, &[take.subject], span);
}

/// The type of the optional a take of `take` into `taken` empties:
/// `taken`'s own type, wrapped once for each optional layer between the
/// hole and the payload. A field of an enum place names no variant, so the
/// type is read off the local taken into rather than off the place.
fn taken_hole_type(ctx: &LoweringContext, take: &BoxedPayload, taken: Local) -> Type {
    let crossed = (take.place.projection.len()).saturating_sub(take.hole.projection.len());
    let mut ty = ctx.body.local_decls[taken.0].ty.clone();
    for _ in 0..crossed {
        let span = ty.span;
        ty = Type::new(TypeKind::Option(Box::new(ty)), span);
    }
    ty
}

/// Ends each of `locals` as consumed, where a keeper takes a payload they
/// hold or read.
fn end_consumed(ctx: &mut LoweringContext, locals: &[Local], span: Span) {
    for &local in locals {
        ctx.push_statement(MirStatement {
            kind: MirStatementKind::StorageDead(Place::new(local), EndCause::Consumed),
            span,
        });
    }
}

/// The payload of the optional `optional`.
fn optional_payload(optional: Local) -> Place {
    Place {
        local: optional,
        projection: vec![PlaceElem::Field(0)],
    }
}

/// The local `operand` moves whole, if it moves one.
fn moved_whole_local(operand: &Operand) -> Option<Local> {
    match operand {
        Operand::Move(place) if place.projection.is_empty() => Some(place.local),
        Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) | Operand::Function(_) => None,
    }
}

/// Ends, before the call, the scrutinee each payload binding the call hands
/// over by `move` was matched out of, when the type checker records the
/// scrutinee as the scope's own ([`TypeChecker::ended_scrutinee`]): the
/// keeper then holds the payload's only reference, and its hook runs where
/// the keeper's new owner ends. A scrutinee two payloads handed to the one
/// call were bound out of ends once, and one `handed` ended already
/// ([`hand_over_matched_payloads`]) is left alone. `consumed` pairs each
/// consumed operand's expression id with the local it read; `moved` lists
/// the locals handed over.
fn end_matched_scrutinees(
    ctx: &mut LoweringContext,
    call: &KeptCall<'_>,
    (consumed, moved): (&[(usize, Local)], &[Local]),
    handed: &[usize],
) {
    let mut ended: Vec<usize> = handed.to_vec();
    let operands =
        (call.receiver.into_iter()).chain(call.args.iter().map(variable::argument_value));
    for operand in operands {
        let Some(scrutinee) = ctx.type_checker.ended_scrutinee(operand) else {
            continue;
        };
        let is_moved =
            (consumed.iter()).any(|&(id, local)| id == operand.id && moved.contains(&local));
        if !is_moved || ended.contains(&scrutinee) {
            continue;
        }
        ended.push(scrutinee);
        let holders = ctx.matched_scrutinees.get(&scrutinee).cloned();
        for holder in holders.into_iter().flatten() {
            if ctx.is_counted(&ctx.body.local_decls[holder.0].ty.kind) {
                ctx.push_statement(MirStatement {
                    kind: MirStatementKind::StorageDead(Place::new(holder), EndCause::Consumed),
                    span: call.span,
                });
            }
        }
    }
}

/// The binding of the caller's a call through a lending parameter is handed
/// at `position` ([`crate::type_checker::TypeChecker::is_lent_argument`]): the function value owns
/// it but only reads it, so the call hands it a copy it funds, spelled as a
/// temporary of its own, and the binding stays the caller's. A temporary only
/// the call releases is handed over as any other.
fn lent_local(
    ctx: &LoweringContext,
    call: &KeptCall<'_>,
    position: usize,
    arg: &Operand,
) -> Option<Local> {
    let written = (call.receiver.into_iter()).chain(call.args).nth(position)?;
    let is_lent = ctx
        .type_checker
        .is_lent_argument(variable::argument_value(written));
    let local = whole_local_copied(arg).filter(|_| is_lent)?;
    (!is_call_temporary(ctx, call, local)).then_some(local)
}

/// The local a call hands over by `move` at an owned position whose argument
/// is `arg`: a binding of the caller's the call consumes, a temporary only
/// the call releases, or, inside an instantiation, a funded copy of a
/// binding the type checker saw as a value of an open type parameter, which
/// it lends; `None` when the argument is copied and funded where it stands.
fn handed_over_local(
    ctx: &mut LoweringContext,
    call: &KeptCall<'_>,
    consumed: &[(usize, Local)],
    arg: &Operand,
) -> Option<Local> {
    let local = whole_local_copied(arg)?;
    let is_own_binding = variable::is_own_binding(ctx, local);
    let is_consumed = consumed.iter().any(|&(_, read)| read == local);
    if (is_consumed && is_own_binding) || is_call_temporary(ctx, call, local) {
        return Some(local);
    }
    (is_own_binding && is_lowering_an_instantiation(ctx))
        .then(|| funded_copy(ctx, local, call.span))
}

/// The argument expressions of `call` in operand order, the receiver first,
/// which [`check_funded_argument`] checks; `None` when they are not checked:
/// in a body compiled per copy, which funds what its shared body lends, and
/// in a call whose operands do not line up with its arguments as written (a
/// named argument).
fn funded_operands<'c>(ctx: &LoweringContext, call: &KeptCall<'c>) -> Option<Vec<&'c Expression>> {
    if is_lowering_an_instantiation(ctx) || ctx.lowers_trait_default {
        return None;
    }
    let is_named = |arg: &Expression| matches!(arg.node, ExpressionKind::NamedArgument(..));
    if call.args.iter().any(is_named) {
        return None;
    }
    Some(call.receiver.into_iter().chain(call.args).collect())
}

/// Checks `argument`, which a call copies at an owned position and funds
/// with a retain rather than taking over: the type checker consumed the
/// binding it names, or recorded it as a resource a keeper is handed a
/// funded copy of ([`TypeChecker::is_funded_copy`]). A value that is no
/// resource is shared, not held twice, and is not checked. `consumed` pairs
/// each consumed operand's expression id with its local.
///
/// Refused otherwise: a resource read out of a place reaching a position
/// that keeps it, which the type checker refuses, would have two holders.
fn check_funded_argument(
    ctx: &LoweringContext,
    span: Span,
    argument: Option<&Expression>,
    consumed: &[(usize, Local)],
) -> Result<(), LoweringError> {
    let Some(argument) = argument else {
        return Ok(());
    };
    let is_accounted = consumed.iter().any(|&(id, _)| id == argument.id)
        || ctx.type_checker.is_funded_copy(argument);
    let is_resource = (ctx.recorded_type(argument.id))
        .is_some_and(|ty| ctx.type_checker.resource_types().holds(&ty.kind));
    if is_accounted || !is_resource {
        return Ok(());
    }
    let message = "a resource read out of a place reaches a position that keeps it";
    Err(internal_error(message, span))
}

/// Whether the body being lowered is an instantiation of a generic body,
/// whose consumption the type checker decided at the open type parameters:
/// a value of one is lent there, so a binding the instantiation's types make
/// a resource is never consumed, and may be read again after the call.
fn is_lowering_an_instantiation(ctx: &LoweringContext) -> bool {
    !ctx.generic_subs.is_empty()
}

/// A temporary holding a copy of `local`, which reference counting funds
/// with a retain: the reference a call hands over when the caller keeps its
/// own binding.
fn funded_copy(ctx: &mut LoweringContext, local: Local, span: Span) -> Local {
    let ty = ctx.body.local_decls[local.0].ty.clone();
    let copy = ctx.push_temp(ty, span);
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::Assign(
            Place::new(copy),
            Rvalue::Use(Operand::Copy(Place::new(local))),
        ),
        span,
    });
    copy
}

/// Marks each argument at a position `kept` marks [`ArgMode::Owned`], and
/// hands over by `move` each one `take` answers a local for, returning the
/// locals handed over. Refused when the call passes fewer arguments than the
/// positions `kept` lists, or hands one local over twice.
fn pass_owned(
    kept: &[bool],
    (arg_ops, arg_modes): (&mut [Operand], &mut [ArgMode]),
    span: Span,
    mut take: impl FnMut(usize, &Operand) -> Result<Option<Local>, LoweringError>,
) -> Result<Vec<Local>, LoweringError> {
    if arg_ops.len() < kept.len() || arg_modes.len() < kept.len() {
        let message = "a call passes fewer arguments than its callee owns parameters";
        return Err(internal_error(message, span));
    }
    let mut moved = Vec::new();
    for (position, _) in (kept.iter().enumerate()).filter(|(_, is_kept)| **is_kept) {
        arg_modes[position] = ArgMode::Owned;
        let Some(local) = take(position, &arg_ops[position])? else {
            continue;
        };
        if moved.contains(&local) {
            let message = "a call hands the same local over at two owned positions";
            return Err(internal_error(message, span));
        }
        arg_ops[position] = Operand::Move(Place::new(local));
        moved.push(local);
    }
    Ok(moved)
}

fn internal_error(message: &str, span: Span) -> LoweringError {
    LoweringError::internal(DiagnosticCode::MirValidationFailed, message, span)
}

/// The local `operand` copies whole, if it copies one.
fn whole_local_copied(operand: &Operand) -> Option<Local> {
    match operand {
        Operand::Copy(place) if place.projection.is_empty() => Some(place.local),
        Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) | Operand::Function(_) => None,
    }
}

/// Passes each argument at a position the forwarding body's target keeps,
/// `kept`, [`ArgMode::Owned`], by `move` of the local the body bound that
/// parameter to, `owned`, which the body gives up to the target. Returns
/// what the call took over, which [`end_handed_over`] ends after it; every
/// owned local the target does not take ends where the body's owned locals
/// do, releasing what the body was handed and its target did not keep.
///
/// Refused when the target keeps a parameter the body does not own: the
/// value's convention owns every position a function keeps.
pub(crate) fn hand_over_owned_locals(
    kept: &[bool],
    owned: &[Option<Local>],
    (arg_ops, arg_modes): (&mut [Operand], &mut [ArgMode]),
    span: Span,
) -> Result<HandedOver, LoweringError> {
    let taken = |position: usize, arg: &Operand| {
        let Some(&Some(local)) = owned.get(position) else {
            let message = "a forwarding body hands its target a parameter it does not own";
            return Err(internal_error(message, span));
        };
        Ok((whole_local_copied(arg) == Some(local)).then_some(local))
    };
    let moved = pass_owned(kept, (arg_ops, arg_modes), span, taken)?;
    Ok(HandedOver {
        moved,
        consumed: Vec::new(),
    })
}

/// The call a forwarding body makes to the body it adapts: a function value's
/// forwarding thunk, or the adapter a vtable slot reaches a body through.
pub(crate) struct ForwardedCall<'k> {
    pub func: Operand,
    /// The arguments, each the parameter the forwarding body was passed or
    /// the local it bound an owned one to.
    pub args: Vec<Operand>,
    pub arg_modes: Vec<ArgMode>,
    /// Which parameters the target owns, in operand order; empty when it
    /// owns none.
    pub target_kept: &'k [bool],
    /// The local each parameter the forwarding body owns was bound to.
    pub owned_locals: &'k [Option<Local>],
}

/// Calls the target of a forwarding body into its return local, handing the
/// target, by `move`, each parameter it keeps ([`hand_over_owned_locals`]),
/// and ends, once it returns, what the target took; lowering continues after
/// the call. Every owned local the target did not take ends where the
/// forwarding body's owned locals do, releasing what the body was handed and
/// its target did not keep.
pub(crate) fn emit_forwarded_call(
    ctx: &mut LoweringContext,
    call: ForwardedCall<'_>,
    span: Span,
) -> Result<(), LoweringError> {
    let ForwardedCall {
        func,
        mut args,
        mut arg_modes,
        target_kept,
        owned_locals,
    } = call;
    let handed =
        hand_over_owned_locals(target_kept, owned_locals, (&mut args, &mut arg_modes), span)?;
    let after_call = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func,
            args,
            arg_modes,
            arg_handles: Vec::new(),
            flags: CallFlags::default(),
            destination: Place::new(RETURN_LOCAL),
            target: Some(after_call),
        },
        span,
    ));
    ctx.set_current_block(after_call);
    end_handed_over(ctx, &handed, span);
    Ok(())
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

/// Refuses a call lowered without its callee's convention — one lending
/// every operand — to the method `method` of `owner` when that method owns a
/// parameter, which it would release though no caller handed it one.
///
/// Every call an operator or a `for` loop makes follows its callee's
/// convention ([`hand_over_kept_arguments`]). Two sites still lend, a string
/// concatenation and a zero-argument method read as a property, and this
/// holds each to a callee owning nothing.
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
