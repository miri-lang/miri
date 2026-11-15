// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rule that every argument a call or a virtual call hands over
//! by copy is funded by a retain reaching the call on every path.
//!
//! `main` passes `_1.0`, the node its holder holds, to a call that owns it.

use crate::mir::analysis::heap_effects::{
    block, body_of, call_into, field, function, holder, local, node, returning, span, statement,
    store,
};
use miri::mir::verify::verify_owned_copies;
use miri::mir::{ArgMode, BasicBlock, Body, Discriminant, Operand, StatementKind, TerminatorKind};

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

/// `main` running `blocks` over a holder `_1` and a node `_2`.
fn main_of(blocks: Vec<miri::mir::BasicBlockData>) -> Body {
    body_of(
        &[miri::ast::types::TypeKind::Void, holder(), node()],
        0,
        blocks,
    )
}

fn goto(target: usize) -> TerminatorKind {
    TerminatorKind::Goto {
        target: BasicBlock(target),
    }
}

/// A direct call handing over `_1.0`, continuing to `target`.
fn handing_over_field_to(target: usize) -> TerminatorKind {
    let mut call = direct_call(Operand::Copy(field(1, 0)));
    if let TerminatorKind::Call { target: next, .. } = &mut call {
        *next = Some(BasicBlock(target));
    }
    call
}

/// A retain in a block every path to the call runs funds the copy, though a
/// pass split it from the block the call ends.
#[test]
fn a_field_retained_in_a_block_dominating_the_call_passes() {
    let retain = statement(StatementKind::IncRef(field(1, 0)));
    let found = verify_owned_copies(&main_of(vec![
        block(vec![retain], goto(1)),
        block(Vec::new(), handing_over_field_to(2)),
        returning(Vec::new()),
    ]));
    assert!(found.is_empty(), "{found:?}");
}

/// A write to the holder in a block between the retain and the call funds
/// the node the write replaced.
#[test]
fn a_field_written_in_a_block_between_its_retain_and_the_call_is_reported() {
    let retain = statement(StatementKind::IncRef(field(1, 0)));
    let found = verify_owned_copies(&main_of(vec![
        block(vec![retain], goto(1)),
        block(vec![store(field(1, 0), local(2))], goto(2)),
        block(Vec::new(), handing_over_field_to(3)),
        returning(Vec::new()),
    ]));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].message.contains("call ending bb2"),
        "{}",
        found[0].message
    );
}

/// A retain an earlier call on the path handed over funds that call alone:
/// the next call handing the field over needs a retain of its own.
#[test]
fn a_retain_an_earlier_call_handed_over_funds_no_later_call() {
    let retain = statement(StatementKind::IncRef(field(1, 0)));
    let found = verify_owned_copies(&main_of(vec![
        block(vec![retain], handing_over_field_to(1)),
        block(Vec::new(), handing_over_field_to(2)),
        returning(Vec::new()),
    ]));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].message.contains("call ending bb1"),
        "{}",
        found[0].message
    );
}

fn retain() -> miri::mir::Statement {
    statement(StatementKind::IncRef(field(1, 0)))
}

fn release() -> miri::mir::Statement {
    statement(StatementKind::DecRef(field(1, 0)))
}

/// A branch to `then` or `otherwise`.
fn branch(then: usize, otherwise: usize) -> TerminatorKind {
    TerminatorKind::SwitchInt {
        discr: Operand::Copy(local(2)),
        targets: vec![(Discriminant::bool_true(), BasicBlock(then))],
        otherwise: BasicBlock(otherwise),
    }
}

/// A release of the field after its retain gives the retain back: the call
/// handing the field over afterwards is unfunded.
#[test]
fn a_field_released_after_its_retain_in_the_call_block_is_reported() {
    let found = verify_owned_copies(&main_of(vec![
        block(vec![retain(), release()], handing_over_field_to(1)),
        returning(Vec::new()),
    ]));
    assert_eq!(found.len(), 1, "{found:?}");
}

/// The same release in a block split between the retain and the call.
#[test]
fn a_field_released_in_a_block_between_its_retain_and_the_call_is_reported() {
    let found = verify_owned_copies(&main_of(vec![
        block(vec![retain()], goto(1)),
        block(vec![release()], goto(2)),
        block(Vec::new(), handing_over_field_to(3)),
        returning(Vec::new()),
    ]));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].message.contains("call ending bb2"),
        "{}",
        found[0].message
    );
}

/// A retain on one branch of a diamond funds no call after the join: the
/// other branch reaches it with none.
#[test]
fn a_field_retained_on_one_branch_only_is_reported_after_the_join() {
    let found = verify_owned_copies(&main_of(vec![
        block(Vec::new(), branch(1, 2)),
        block(vec![retain()], goto(3)),
        block(Vec::new(), goto(3)),
        block(Vec::new(), handing_over_field_to(4)),
        returning(Vec::new()),
    ]));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].message.contains("call ending bb3"),
        "{}",
        found[0].message
    );
}

/// A retain on both branches funds the call after the join.
#[test]
fn a_field_retained_on_both_branches_passes_after_the_join() {
    let found = verify_owned_copies(&main_of(vec![
        block(Vec::new(), branch(1, 2)),
        block(vec![retain()], goto(3)),
        block(vec![retain()], goto(3)),
        block(Vec::new(), handing_over_field_to(4)),
        returning(Vec::new()),
    ]));
    assert!(found.is_empty(), "{found:?}");
}

/// A retain made once before a loop funds the first iteration's call alone:
/// the second reaches the call with none.
#[test]
fn a_retain_before_a_loop_funds_no_call_in_its_body() {
    let found = verify_owned_copies(&main_of(vec![
        block(vec![retain()], goto(1)),
        block(Vec::new(), handing_over_field_to(2)),
        block(Vec::new(), branch(1, 3)),
        returning(Vec::new()),
    ]));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].message.contains("call ending bb1"),
        "{}",
        found[0].message
    );
}

/// A block no path reaches funds nothing: its retain does not reach the
/// call it jumps to.
#[test]
fn a_retain_in_an_unreachable_predecessor_funds_no_call() {
    let found = verify_owned_copies(&main_of(vec![
        block(Vec::new(), goto(2)),
        block(vec![retain()], goto(2)),
        block(Vec::new(), handing_over_field_to(3)),
        returning(Vec::new()),
    ]));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].message.contains("call ending bb2"),
        "{}",
        found[0].message
    );
}

/// A call no path reaches never runs, so it hands nothing over.
#[test]
fn an_unreachable_call_is_not_reported() {
    let found = verify_owned_copies(&main_of(vec![
        returning(Vec::new()),
        block(Vec::new(), handing_over_field_to(0)),
    ]));
    assert!(found.is_empty(), "{found:?}");
}
