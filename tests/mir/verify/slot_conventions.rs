// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rules that a call through a vtable slot, every body the slot
//! is filled with and every adapter a slot reaches a body through agree on
//! what the call hands over.
//!
//! Slot 1 stands for `take(self Holder, n Node)` and owns `n`. Each rule is
//! shown reporting the defect it guards, and passing the program lowering
//! writes.

use std::collections::{HashMap, HashSet};

use crate::mir::analysis::heap_effects::{
    block, body_of, call_into, copy, function, holder, local, node, returning, span, ty,
};
use miri::ast::types::TypeKind;
use miri::mir::facts::ProgramFacts;
use miri::mir::lowering::vtable_demand::{FilledSlot, VtableFills};
use miri::mir::symbol::Symbol;
use miri::mir::verify::{verify_slot_conventions, SlotSpecification, VerificationViolation};
use miri::mir::{ArgMode, BasicBlock, Body, Operand, TerminatorKind};

const VTABLE: &str = "miri.Impl.$vtable";

/// Slot 1 owns its second position, the parameter after the receiver.
fn conventions() -> HashMap<usize, Vec<bool>> {
    HashMap::from([(1, vec![false, true])])
}

/// `take(self Holder, n Node)`, owning the parameters `kept` marks.
fn take(kept: &[bool]) -> Body {
    let mut body = body_of(
        &[TypeKind::Void, holder(), node()],
        2,
        vec![returning(Vec::new())],
    );
    body.signature.kept_params = kept.to_vec();
    body
}

/// `main`, calling `take` through slot 1 on its locals, passing them as
/// `modes` says.
fn virtual_caller(modes: Vec<ArgMode>) -> Body {
    body_of(
        &[TypeKind::Void, holder(), node()],
        0,
        vec![
            block(
                Vec::new(),
                TerminatorKind::VirtualCall {
                    vtable_slot: 1,
                    args: vec![copy(local(1)), copy(local(2))],
                    arg_modes: modes,
                    param_types: vec![Some(ty(node()))],
                    destination: local(0),
                    target: Some(BasicBlock(1)),
                },
            ),
            returning(Vec::new()),
        ],
    )
}

/// An adapter body calling each function `callees` names, in turn.
fn adapter_calling(callees: &[Symbol]) -> Body {
    let mut blocks: Vec<_> = (callees.iter().enumerate())
        .map(|(index, callee)| {
            let call = call_into(
                Operand::function(callee.clone(), span()),
                vec![copy(local(1)), copy(local(2))],
                local(0),
                index + 1,
            );
            block(Vec::new(), call)
        })
        .collect();
    blocks.push(returning(Vec::new()));
    let mut body = body_of(&[TypeKind::Void, holder(), node()], 2, blocks);
    body.signature.kept_params = vec![false, true];
    body
}

/// Slot 1 of the one vtable filled with `symbol`.
fn fills_with(symbol: &Symbol) -> VtableFills {
    let slot = FilledSlot {
        slot: 1,
        method: "take".to_string(),
        symbol: symbol.clone(),
    };
    [(VTABLE.to_string(), vec![slot])].into_iter().collect()
}

fn violations(
    program: Vec<(Symbol, Body)>,
    filled: &Symbol,
) -> Vec<(Symbol, VerificationViolation)> {
    let facts = ProgramFacts::of(&program);
    let conventions = conventions();
    let fills = fills_with(filled);
    let withheld = HashSet::new();
    let specification = SlotSpecification {
        conventions: &conventions,
        fills: &fills,
        withheld: &withheld,
    };
    verify_slot_conventions(&program, &facts, &specification)
}

fn messages(found: &[(Symbol, VerificationViolation)]) -> Vec<&str> {
    found
        .iter()
        .map(|(_, found)| found.message.as_str())
        .collect()
}

/// A virtual call lending a position its slot owns would leave the body it
/// reaches releasing a reference its caller still holds.
#[test]
fn a_virtual_call_lending_a_position_its_slot_owns_is_reported() {
    let program = vec![
        (function("take"), take(&[false, true])),
        (Symbol::entry(), virtual_caller(Vec::new())),
    ];
    let found = violations(program, &function("take"));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, Symbol::entry());
    assert!(
        found[0]
            .1
            .message
            .contains("a call through vtable slot 1 lends argument 1, which the slot owns"),
        "{:?}",
        messages(&found)
    );
}

/// A virtual call handing over a position its slot lends gives the body a
/// reference nobody releases.
#[test]
fn a_virtual_call_handing_over_a_position_its_slot_lends_is_reported() {
    let program = vec![
        (function("take"), take(&[false, true])),
        (
            Symbol::entry(),
            virtual_caller(vec![ArgMode::Owned, ArgMode::Owned]),
        ),
    ];
    let found = violations(program, &function("take"));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0]
            .1
            .message
            .contains("a call through vtable slot 1 hands over argument 0, which the slot lends"),
        "{:?}",
        messages(&found)
    );
}

/// The call lowering writes, handing over exactly what the slot owns to a
/// body owning exactly that, breaks no rule.
#[test]
fn a_virtual_call_and_a_body_following_the_slots_convention_pass() {
    let program = vec![
        (function("take"), take(&[false, true])),
        (
            Symbol::entry(),
            virtual_caller(vec![ArgMode::Borrowed, ArgMode::Owned]),
        ),
    ];
    let found = violations(program, &function("take"));
    assert!(found.is_empty(), "{:?}", messages(&found));
}

/// A body filling a slot that owns a parameter, while the body lends it,
/// would be handed a reference it never releases.
#[test]
fn a_body_filling_a_slot_without_owning_what_the_slot_owns_is_reported() {
    let program = vec![(function("take"), take(&[]))];
    let found = violations(program, &function("take"));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, function("take"));
    assert!(
        found[0].1.message.contains(
            "fills slot 1 of `miri.Impl.$vtable` but lends parameter 1, which the slot owns"
        ),
        "{:?}",
        messages(&found)
    );
}

/// A body owning a parameter its slot lends would release a reference every
/// caller through the slot still holds.
#[test]
fn a_body_filling_a_slot_owning_what_the_slot_lends_is_reported() {
    let program = vec![(function("take"), take(&[true, true]))];
    let found = violations(program, &function("take"));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].1.message.contains(
            "fills slot 1 of `miri.Impl.$vtable` but owns parameter 0, which the slot lends"
        ),
        "{:?}",
        messages(&found)
    );
}

/// An adapter whose call reaches another function than the one it adapts
/// runs a body the slot was never filled with.
#[test]
fn a_slot_adapter_calling_another_function_than_its_target_is_reported() {
    let adapter = Symbol::slot_adapter(&function("take"), &[false, true]);
    let program = vec![
        (function("take"), take(&[])),
        (function("other"), take(&[])),
        (adapter.clone(), adapter_calling(&[function("other")])),
    ];
    let found = violations(program, &adapter);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, adapter);
    let message = &found[0].1.message;
    assert!(
        message.contains("but calls `") && message.contains("instead, a body the slot was never"),
        "{message}"
    );
}

/// An adapter calling its target twice runs the body twice per call.
#[test]
fn a_slot_adapter_calling_its_target_twice_is_reported() {
    let adapter = Symbol::slot_adapter(&function("take"), &[false, true]);
    let target = function("take");
    let program = vec![
        (target.clone(), take(&[])),
        (adapter.clone(), adapter_calling(&[target.clone(), target])),
    ];
    let found = violations(program, &adapter);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0]
            .1
            .message
            .contains("but makes 2 calls, where it calls `"),
        "{:?}",
        messages(&found)
    );
}

/// The adapter lowering writes makes one call, to the body it adapts.
#[test]
fn a_slot_adapter_calling_its_target_once_passes() {
    let adapter = Symbol::slot_adapter(&function("take"), &[false, true]);
    let target = function("take");
    let program = vec![
        (target.clone(), take(&[])),
        (adapter.clone(), adapter_calling(&[target])),
    ];
    let found = violations(program, &adapter);
    assert!(found.is_empty(), "{:?}", messages(&found));
}

/// Slot 2 owns the receiver of `take` and lends its parameter.
fn receiver_conventions() -> HashMap<usize, Vec<bool>> {
    HashMap::from([(2, vec![true, false])])
}

/// `main`, calling through slot 2 on its locals, passing them as `modes`
/// says.
fn receiver_caller(modes: Vec<ArgMode>) -> Body {
    let mut main = virtual_caller(modes);
    if let Some(terminator) = main.basic_blocks[0].terminator.as_mut() {
        if let TerminatorKind::VirtualCall { vtable_slot, .. } = &mut terminator.kind {
            *vtable_slot = 2;
        }
    }
    main
}

fn receiver_violations(main: Body) -> Vec<(Symbol, VerificationViolation)> {
    let program = vec![(Symbol::entry(), main)];
    let facts = ProgramFacts::of(&program);
    let conventions = receiver_conventions();
    let fills = VtableFills::default();
    let withheld = HashSet::new();
    let specification = SlotSpecification {
        conventions: &conventions,
        fills: &fills,
        withheld: &withheld,
    };
    verify_slot_conventions(&program, &facts, &specification)
}

/// A slot owning the receiver is handed it at position 0: a call lending
/// the receiver there is reported.
#[test]
fn a_virtual_call_lending_a_receiver_its_slot_owns_is_reported() {
    let found = receiver_violations(receiver_caller(Vec::new()));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0]
            .1
            .message
            .contains("a call through vtable slot 2 lends argument 0, which the slot owns"),
        "{:?}",
        messages(&found)
    );
}

/// A call handing the receiver over where its slot owns it passes.
#[test]
fn a_virtual_call_handing_over_a_receiver_its_slot_owns_passes() {
    let found = receiver_violations(receiver_caller(vec![ArgMode::Owned]));
    assert!(found.is_empty(), "{:?}", messages(&found));
}

/// A slot naming a body the program does not define, and that is not
/// withheld, is reported rather than skipped.
#[test]
fn a_filled_slot_naming_no_body_of_the_program_is_reported() {
    let found = violations(Vec::new(), &function("take"));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0]
            .1
            .message
            .contains("is no body of the program and the slot is not withheld"),
        "{:?}",
        messages(&found)
    );
}

/// A withheld slot keeps the runtime trap and names no body: not reported.
#[test]
fn a_withheld_slot_naming_no_body_passes() {
    let conventions = conventions();
    let fills = fills_with(&function("take"));
    let withheld = HashSet::from([function("take")]);
    let specification = SlotSpecification {
        conventions: &conventions,
        fills: &fills,
        withheld: &withheld,
    };
    let program: Vec<(Symbol, Body)> = Vec::new();
    let facts = ProgramFacts::of(&program);
    let found = verify_slot_conventions(&program, &facts, &specification);
    assert!(found.is_empty(), "{:?}", messages(&found));
}

/// An adapter owning less than its target keeps could hand that parameter
/// over only by a retain, which an adapter never makes.
#[test]
fn a_slot_adapter_owning_less_than_its_target_keeps_is_reported() {
    let adapter = Symbol::slot_adapter(&function("take"), &[false, false]);
    let target = function("take");
    let mut adapter_body = adapter_calling(std::slice::from_ref(&target));
    adapter_body.signature.kept_params = Vec::new();
    let program = vec![
        (target.clone(), take(&[false, true])),
        (adapter.clone(), adapter_body),
    ];
    let found: Vec<_> = violations(program, &target)
        .into_iter()
        .filter(|(symbol, _)| *symbol == adapter)
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0]
            .1
            .message
            .contains("does not own parameter 1, which `"),
        "{:?}",
        messages(&found)
    );
}
