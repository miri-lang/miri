// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The adapter a vtable slot reaches a body through when the body keeps less
//! than the slot hands.
//!
//! A call through a slot hands over every position the slot owns
//! ([`crate::type_checker::slot_conventions`]), read at the types it passes.
//! A body filling the slot that owns fewer of those positions is not put in
//! the slot itself: the slot points at an adapter with the body's signature
//! that owns exactly what the slot hands, calls the body handing it what it
//! keeps ([`emit_forwarded_call`]), and releases the rest where the adapter
//! ends, before control returns to the caller. A slot hands at least as much
//! as any body filling it keeps, so an adapter never retains.
//!
//! An adapter is named after the body it calls and what it owns
//! ([`Symbol::slot_adapter`]), so a body compiled per instantiation of a
//! generic class is adapted once per instantiation, and two slots handing
//! one body the same positions share one adapter.

use std::collections::{HashMap, HashSet};

use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::dispatch::{VtableFills, VtableLayout};
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::kept_parameters::{
    emit_forwarded_call, record_and_bind_owned_parameters, slot_convention_at, ForwardedCall,
};
use crate::mir::symbol::{Symbol, SymbolTable};
use crate::mir::{
    ArgMode, Body, ExecutionModel, Local, LocalDecl, Operand, Place, Terminator, TerminatorKind,
};
use crate::type_checker::TypeChecker;

/// What adapting the filled slots reads: the slot numbering, the bodies
/// lowered so far by their symbols, and the targets withheld at their
/// instance, whose slots keep the runtime trap.
pub struct SlotBodies<'b> {
    pub layout: &'b VtableLayout,
    pub bodies: &'b HashMap<&'b Symbol, &'b Body>,
    pub withheld: &'b HashSet<Symbol>,
}

/// Points every slot of `fills` whose body keeps less than the slot hands at
/// an adapter, and returns the body of each adapter `symbols` had not
/// claimed yet, lowered once whatever number of slots reach it. Runs once the
/// vtables are settled and every body a slot names is lowered; an adapter
/// calls only its target, so it reaches nothing new.
pub fn adapt_filled_slots(
    type_checker: &TypeChecker,
    fills: &mut VtableFills,
    reach: &SlotBodies<'_>,
    (symbols, is_release): (&mut SymbolTable, bool),
) -> Result<Vec<(Symbol, Body)>, LoweringError> {
    let mut adapters = Vec::new();
    for filled in fills.slots_mut() {
        if reach.withheld.contains(&filled.symbol) {
            continue;
        }
        let Some(&target) = reach.bodies.get(&filled.symbol) else {
            continue;
        };
        let slot = (reach.layout, filled.slot);
        let Some(owned) = slot_adapter_convention(type_checker, slot, target)? else {
            continue;
        };
        let adapter = Symbol::slot_adapter(&filled.symbol, &owned);
        filled.symbol = adapter.clone();
        if symbols.claim_at(&adapter, target.span)? {
            let body = lower_slot_adapter(type_checker, &adapter, target, is_release)?;
            adapters.push((adapter, body));
        }
    }
    Ok(adapters)
}

/// The positions an adapter between the slot numbered `slot` and the body
/// `target` filling it owns, or `None` when the body owns exactly what the
/// slot hands and fills the slot itself.
///
/// Refused when the body owns a position the slot does not hand, which an
/// adapter could only fund by a retain.
pub fn slot_adapter_convention(
    type_checker: &TypeChecker,
    (layout, slot): (&VtableLayout, usize),
    target: &Body,
) -> Result<Option<Vec<bool>>, LoweringError> {
    let span = target.span;
    let Some((root, selector)) = layout.key(slot) else {
        let message = format!("a filled vtable slot, {slot}, is no slot of the layout");
        return Err(internal_error(message, span));
    };
    let parameter_types = (target.local_decls.iter().skip(1))
        .take(target.arg_count)
        .map(|decl| &decl.ty);
    let handed = slot_convention_at((type_checker, target), (root, selector), parameter_types);
    let kept = &target.signature.kept_params;
    let is_handed = |position: usize| handed.get(position).copied().unwrap_or(false);
    let is_kept = |position: usize| kept.get(position).copied().unwrap_or(false);
    let positions = handed.len().max(kept.len());
    if (0..positions).any(|position| is_kept(position) && !is_handed(position)) {
        let message = format!(
            "a body filling the vtable slot of `{root}.{selector}` keeps a parameter the slot \
             does not hand over"
        );
        return Err(internal_error(message, span));
    }
    let is_adapted = (0..positions).any(|position| is_handed(position) != is_kept(position));
    Ok(is_adapted.then_some(handed))
}

/// The body of the adapter `adapter` names ([`Symbol::slot_adapter`]),
/// calling `target`, the body its slot is filled with: the target's
/// signature, owning the positions the adapter's symbol lists, each bound on
/// entry to a local of its own; one call to the target, handing it by
/// `move` each position it keeps and lending the rest; and the release, at
/// its end, of every owned local the target did not take.
pub fn lower_slot_adapter(
    type_checker: &TypeChecker,
    adapter: &Symbol,
    target: &Body,
    is_release: bool,
) -> Result<Body, LoweringError> {
    let span = target.span;
    let Some((target_symbol, owned)) = adapter.slot_adapter_target() else {
        let message = format!("`{adapter}` names no vtable slot adapter");
        return Err(internal_error(message, span));
    };
    let mut ctx = adapter_context(type_checker, target, is_release);
    let names: Vec<String> = (1..=target.arg_count)
        .map(|index| format!("__slot_argument{index}"))
        .collect();
    for (name, decl) in names.iter().zip(target.local_decls.iter().skip(1)) {
        ctx.push_param(name.clone(), decl.ty.clone(), span);
    }
    let mut kept_params = owned.to_vec();
    kept_params.resize(target.arg_count, false);
    let params = names.iter().map(|name| (name.as_str(), span));
    let (bindings, owned_locals) =
        record_and_bind_owned_parameters(&mut ctx, kept_params, params, span)?;
    let forwarded = ForwardedCall {
        func: Operand::function(target_symbol.clone(), span),
        args: forwarded_arguments(&owned_locals),
        arg_modes: lent_argument_modes(target),
        target_kept: &target.signature.kept_params,
        owned_locals: &owned_locals,
    };
    emit_forwarded_call(&mut ctx, forwarded, span)?;
    bindings.close(&mut ctx, span);
    ctx.set_terminator(Terminator::new(TerminatorKind::Return, span));
    Ok(ctx.body)
}

/// A fresh context for the adapter of `target`: a host body with the
/// target's return slot, signature and instantiation, owning nothing yet.
fn adapter_context<'a>(
    type_checker: &'a TypeChecker,
    target: &Body,
    is_release: bool,
) -> LoweringContext<'a> {
    let span = target.span;
    let mut body = Body::new(target.arg_count, span, ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(target.local_decls[0].ty.clone(), span));
    body.signature = target.signature.clone();
    body.signature.kept_params = Vec::new();
    body.generics = target.generics.clone();
    LoweringContext::new(body, type_checker, is_release)
}

/// The arguments an adapter passes its target: each parameter, `_1` on, or
/// the local the adapter bound an owned one to.
fn forwarded_arguments(owned_locals: &[Option<Local>]) -> Vec<Operand> {
    (owned_locals.iter().enumerate())
        .map(|(index, owned)| Operand::Copy(Place::new(owned.unwrap_or(Local(index + 1)))))
        .collect()
}

/// How an adapter passes each parameter on before it hands its target the
/// ones the target keeps: an `out` parameter as the caller's place, every
/// other one lent.
fn lent_argument_modes(target: &Body) -> Vec<ArgMode> {
    (0..target.arg_count)
        .map(|index| {
            let is_out = target.signature.out_params.get(index).copied();
            ArgMode::from_out_flag(is_out.unwrap_or(false))
        })
        .collect()
}

fn internal_error(message: String, span: Span) -> LoweringError {
    LoweringError::internal(DiagnosticCode::MirValidationFailed, message, span)
}
