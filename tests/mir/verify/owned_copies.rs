// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rule that every argument a call or a virtual call hands over
//! by copy is funded by a retain in the block the call ends.
//!
//! `main` passes `_1.0`, the node its holder holds, to a call that owns it.

use crate::mir::analysis::heap_effects::{
    block, body_of, call_into, field, function, holder, local, node, returning, span, statement,
    store,
};
use miri::mir::verify::verify_owned_copies;
use miri::mir::{ArgMode, BasicBlock, Body, Operand, StatementKind, TerminatorKind};

/// `main`, ending its first block with `call` after `setup`.
fn main_calling(setup: Vec<miri::mir::Statement>, call: TerminatorKind) -> Body {
    body_of(
        &[miri::ast::types::TypeKind::Void, holder(), node()],
        0,
        vec![block(setup, call), returning(Vec::new())],
    )
}

/// A virtual call through slot 1 handing over `arg`.
fn virtual_call(arg: Operand) -> TerminatorKind {
    TerminatorKind::VirtualCall {
        vtable_slot: 1,
        args: vec![Operand::Copy(local(1)), arg],
        arg_modes: vec![ArgMode::Borrowed, ArgMode::Owned],
        param_types: vec![None],
        destination: local(0),
        target: Some(BasicBlock(1)),
    }
}

/// A direct call handing over `arg`.
fn direct_call(arg: Operand) -> TerminatorKind {
    let mut call = call_into(
        Operand::function(function("take"), span()),
        vec![arg],
        local(0),
        1,
    );
    if let TerminatorKind::Call { arg_modes, .. } = &mut call {
        *arg_modes = vec![ArgMode::Owned];
    }
    call
}

/// A field handed over by copy through a virtual call with no retain would
/// be released by the callee while its holder still holds it.
#[test]
fn a_field_a_virtual_call_hands_over_unfunded_is_reported() {
    let found = verify_owned_copies(&main_calling(
        Vec::new(),
        virtual_call(Operand::Copy(field(1, 0))),
    ));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0]
            .message
            .contains("handed over by copy, as `_1.0`, at argument 1 of the call ending bb0"),
        "{}",
        found[0].message
    );
}

/// The same field handed over by a direct call with no retain is reported.
#[test]
fn a_field_a_direct_call_hands_over_unfunded_is_reported() {
    let found = verify_owned_copies(&main_calling(
        Vec::new(),
        direct_call(Operand::Copy(field(1, 0))),
    ));
    assert_eq!(found.len(), 1, "{found:?}");
}

/// A retain of the field in the block funds the copy.
#[test]
fn a_field_handed_over_after_a_retain_passes() {
    let retain = statement(StatementKind::IncRef(field(1, 0)));
    let found = verify_owned_copies(&main_calling(
        vec![retain],
        virtual_call(Operand::Copy(field(1, 0))),
    ));
    assert!(found.is_empty(), "{found:?}");
}

/// A `move` hands the caller's own reference over and needs no retain.
#[test]
fn a_local_handed_over_by_move_passes() {
    let found = verify_owned_copies(&main_calling(
        Vec::new(),
        virtual_call(Operand::Move(local(2))),
    ));
    assert!(found.is_empty(), "{found:?}");
}

/// A direct call handing over each of `args` as `modes` says.
fn direct_call_with(args: Vec<Operand>, modes: Vec<ArgMode>) -> TerminatorKind {
    let mut call = call_into(
        Operand::function(function("take"), span()),
        args,
        local(0),
        1,
    );
    if let TerminatorKind::Call { arg_modes, .. } = &mut call {
        *arg_modes = modes;
    }
    call
}

/// One field handed over twice by copy needs two retains: one retain funds
/// one of the callee's two releases, and the other frees the holder's node.
#[test]
fn a_field_handed_over_twice_with_one_retain_is_reported() {
    let retain = statement(StatementKind::IncRef(field(1, 0)));
    let found = verify_owned_copies(&main_calling(
        vec![retain],
        direct_call_with(
            vec![Operand::Copy(field(1, 0)), Operand::Copy(field(1, 0))],
            vec![ArgMode::Owned, ArgMode::Owned],
        ),
    ));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].message.contains("as `_1.0`, at argument 0"),
        "{}",
        found[0].message
    );
}

/// Two retains fund the same field handed over twice.
#[test]
fn a_field_handed_over_twice_with_two_retains_passes() {
    let retain = || statement(StatementKind::IncRef(field(1, 0)));
    let found = verify_owned_copies(&main_calling(
        vec![retain(), retain()],
        direct_call_with(
            vec![Operand::Copy(field(1, 0)), Operand::Copy(field(1, 0))],
            vec![ArgMode::Owned, ArgMode::Owned],
        ),
    ));
    assert!(found.is_empty(), "{found:?}");
}

/// A write to the holder between the retain and the call funds the node the
/// write replaced, not the one the call hands over.
#[test]
fn a_field_written_between_its_retain_and_the_call_is_reported() {
    let retain = statement(StatementKind::IncRef(field(1, 0)));
    let overwrite = store(field(1, 0), local(2));
    let found = verify_owned_copies(&main_calling(
        vec![retain, overwrite],
        virtual_call(Operand::Copy(field(1, 0))),
    ));
    assert_eq!(found.len(), 1, "{found:?}");
}

/// A counted field a container stores is donated, and the caller funds the
/// reference the container keeps: with no retain it is reported.
#[test]
fn a_counted_field_donated_unfunded_is_reported() {
    let found = verify_owned_copies(&main_calling(
        Vec::new(),
        direct_call_with(vec![Operand::Copy(field(1, 0))], vec![ArgMode::Donated]),
    ));
    assert_eq!(found.len(), 1, "{found:?}");
}

/// A retain funds a donated counted field.
#[test]
fn a_counted_field_donated_after_a_retain_passes() {
    let retain = statement(StatementKind::IncRef(field(1, 0)));
    let found = verify_owned_copies(&main_calling(
        vec![retain],
        direct_call_with(vec![Operand::Copy(field(1, 0))], vec![ArgMode::Donated]),
    ));
    assert!(found.is_empty(), "{found:?}");
}
