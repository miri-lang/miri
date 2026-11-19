// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A copy of an element no judgement refused never compiles either.
//!
//! The type checker refuses a call making a copy of a resource with no
//! `clone` wherever the program reaches it
//! ([`crate::type_checker::copy_judgement`]). That judgement reads the copies
//! of each generic body the program reaches. Every lowered body that is one
//! of those copies is checked again here: a call in it to a `runtime` entry
//! point declared `@clones` or `@copies` is judged at the types the copy
//! binds, by the same predicate, so a reached copy compiled at a type the
//! judgement did not refuse is a compiler defect reported as one, never a
//! resource held twice.
//!
//! TODO: lowering also compiles copies the reached program does not contain
//! (an element's `clone` a container's vtable names, a class instance's
//! methods compiled whole), and leaves them unchecked here. Lowering should
//! compile only the reached copies, so every compiled copy is a judged one.

use std::collections::HashSet;

use crate::ast::literal::Literal;
use crate::ast::types::TypeKind;
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::place_type::PlaceTypes;
use crate::mir::symbol::Symbol;
use crate::mir::{BasicBlock, Body, Operand, TerminatorKind};
use crate::runtime_effects::{ElementRole, ParamUse, ResultUse, RuntimeEffect};
use crate::runtime_fns::rt;
use crate::type_checker::copy_judgement::{copied_element_kinds, refuses_copy_of};
use crate::type_checker::{CopyOwner, ReachedCopy, TypeChecker};

/// The symbol lowering compiles `copy` under.
pub fn reached_copy_symbol(copy: &ReachedCopy) -> Symbol {
    match &copy.owner {
        CopyOwner::Function(module) => Symbol::function(module, &copy.name, &copy.arguments),
        CopyOwner::Type { name, arguments } => {
            Symbol::method(name, arguments, &copy.name, &copy.arguments)
        }
    }
}

/// Refuses each body of `lowered` that is a copy the program reaches and
/// makes a copy of an element `tc` refuses to copy.
///
/// # Errors
///
/// An internal compiler error naming the element type and the copy: the
/// type checker refuses every such copy the program reaches before lowering
/// runs.
pub fn refuse_unjudged_copies(
    tc: &TypeChecker,
    lowered: &[(Symbol, Body)],
) -> Result<(), LoweringError> {
    let reached: HashSet<Symbol> = (tc.reached_program().copies().iter())
        .map(reached_copy_symbol)
        .collect();
    let judged = (lowered.iter()).filter(|(symbol, _)| reached.contains(symbol));
    for (symbol, body) in judged {
        refuse_uncloned_copies_in(tc, symbol, body)?;
    }
    Ok(())
}

/// Refuses the first call in `body`, compiled as `symbol`, that copies an
/// element of a resource type with no `clone`, or one whose copied type
/// the body does not state.
fn refuse_uncloned_copies_in(
    tc: &TypeChecker,
    symbol: &Symbol,
    body: &Body,
) -> Result<(), LoweringError> {
    let calls = (body.basic_blocks.iter())
        .filter_map(|block| block.terminator.as_ref())
        .filter_map(|terminator| {
            let TerminatorKind::Call {
                func, args, target, ..
            } = &terminator.kind
            else {
                return None;
            };
            let is_unsharing = target.is_some_and(|target| is_counted_unshare(body, target));
            (!is_unsharing).then_some((func, args, terminator.span))
        });
    for (func, args, span) in calls {
        let Some(effect) = func.called_runtime_effect() else {
            continue;
        };
        let copied = copied_kinds(tc, body, effect, args)
            .map_err(|position| unjudged_at(position, symbol, span))?;
        let definitions = tc.type_definitions();
        let resources = tc.resource_types();
        let refused = (copied.into_iter())
            .find(|(kind, role)| refuses_copy_of(kind, *role, definitions, resources));
        if let Some((kind, _)) = refused {
            return Err(refused_at(&kind, (symbol, func), span));
        }
    }
    Ok(())
}

/// Whether the block `target`, which a call continues at, reports the copy
/// the call made as one a write makes of a shared value before writing it
/// (`lowering::expression::unshare`). Such a copy runs only on a value two
/// bindings share, and a collection of resources is itself a resource,
/// moved and never shared, so it never copies one.
///
/// TODO: the copy is recognised by the report that follows it; a mark on the
/// call itself, set where the write is lowered, would name it directly.
fn is_counted_unshare(body: &Body, target: BasicBlock) -> bool {
    let Some(terminator) = (body.basic_blocks.get(target.0)).and_then(|b| b.terminator.as_ref())
    else {
        return false;
    };
    let TerminatorKind::Call { func, .. } = &terminator.kind else {
        return false;
    };
    let reported = func.called_symbol().and_then(Symbol::runtime_name);
    reported == Some(rt::RC_STATS_COW_COPY)
}

/// Each value the call with `args` of an entry point doing `effect`
/// copies, with what it is to the collection, at the types `body` binds;
/// the position of an argument whose type the body does not state.
fn copied_kinds(
    tc: &TypeChecker,
    body: &Body,
    effect: &RuntimeEffect,
    args: &[Operand],
) -> Result<Vec<(TypeKind, ElementRole)>, usize> {
    let mut copied = Vec::new();
    for &position in &effect.clones {
        if let ParamUse::LooksUp(role) = effect.param(position) {
            if let Handed::Value(kind) = argument_kind(body, args, position).ok_or(position)? {
                copied.push((kind, role));
            }
        }
    }
    if let ResultUse::Copies(position) = effect.result {
        if let Handed::Value(collection) = argument_kind(body, args, position).ok_or(position)? {
            let elements = copied_element_kinds(&collection, tc.type_definitions());
            copied.extend(
                elements
                    .into_iter()
                    .map(|kind| (kind, ElementRole::Element)),
            );
        }
    }
    Ok(copied)
}

/// What the argument `args` hands at `position` gives a copy to judge.
enum Handed {
    /// A value of this type, as the body's declarations give the place or
    /// constant it reads ([`PlaceTypes`]).
    Value(TypeKind),
    /// The absent value, which holds nothing to copy.
    Absent,
}

/// What the argument `args` hands at `position` is; `None` where the
/// body's declarations give its type nowhere.
fn argument_kind(body: &Body, args: &[Operand], position: usize) -> Option<Handed> {
    let operand = args.get(position)?;
    if let Operand::Constant(constant) = operand {
        if matches!(constant.literal, Literal::None) {
            return Some(Handed::Absent);
        }
    }
    Some(Handed::Value(
        PlaceTypes::of(body).operand_type(operand)?.kind,
    ))
}

/// The internal error for a copy of `kind` the copy `symbol` makes at
/// `span` unrefused, calling `callee`.
fn refused_at(kind: &TypeKind, (symbol, callee): (&Symbol, &Operand), span: Span) -> LoweringError {
    let entry = callee
        .called_symbol()
        .map_or_else(String::new, |entry| format!(" through `{entry}`"));
    LoweringError::internal(
        DiagnosticCode::OwnResourceElementCopied,
        format!(
            "`{symbol}` copies `{kind}`{entry}, a resource with no `clone`, and reached lowering unrefused"
        ),
        span,
    )
}

/// The internal error for a copy the copy `symbol` makes at `span` of its
/// argument at `position`, whose type the body does not state, so the
/// copy cannot be judged.
fn unjudged_at(position: usize, symbol: &Symbol, span: Span) -> LoweringError {
    LoweringError::internal(
        DiagnosticCode::OwnResourceElementCopied,
        format!("`{symbol}` copies its argument {position}, whose type it does not state, so the copy cannot be judged"),
        span,
    )
}
