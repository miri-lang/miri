// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for which value arguments a call needs copied, and for which value
//! parameters a body writes and so copies on entry.

use super::value_bodies::*;
use miri::ast::types::TypeKind;
use miri::mir::value_arguments::{
    copy_value_arguments, value_arguments_needing_copy, written_value_params, Exposure,
};
use miri::mir::{ArgMode, Local, Operand, Rvalue, StatementKind, Terminator, TerminatorKind};
use std::collections::HashSet;

/// The position and reason of every argument of `kind`'s call that needs a
/// copy, made by a caller over [`value_locals`].
fn needing_copy(kind: TerminatorKind) -> Vec<(usize, Exposure)> {
    let body = caller(kind);
    value_arguments_needing_copy(&body, 0)
        .into_iter()
        .map(|argument| (argument.position, argument.exposure))
        .collect()
}

#[test]
fn a_value_argument_the_call_also_writes_as_its_destination_is_copied() {
    assert_eq!(
        needing_copy(call(vec![copy(1)], Vec::new(), place(1))),
        vec![(0, Exposure::Destination)]
    );
}

#[test]
fn a_value_argument_whose_local_holds_the_destination_is_copied() {
    assert_eq!(
        needing_copy(call(vec![copy(1)], Vec::new(), field(1, 0))),
        vec![(0, Exposure::Destination)]
    );
}

#[test]
fn a_value_argument_also_passed_as_out_is_copied() {
    let modes = vec![ArgMode::Out, ArgMode::Borrowed];
    assert_eq!(
        needing_copy(call(vec![copy(1), copy(1)], modes, place(0))),
        vec![(1, Exposure::Out)]
    );
}

#[test]
fn a_value_argument_whose_local_holds_an_out_argument_is_copied() {
    let modes = vec![ArgMode::Out, ArgMode::Borrowed];
    let args = vec![Operand::Copy(field(1, 1)), copy(1)];
    assert_eq!(
        needing_copy(call(args, modes, place(0))),
        vec![(1, Exposure::Out)]
    );
}

#[test]
fn a_projected_value_argument_is_copied() {
    let args = vec![Operand::Copy(field(3, 0))];
    assert_eq!(
        needing_copy(call(args, Vec::new(), place(0))),
        vec![(0, Exposure::Projected)]
    );
}

#[test]
fn distinct_whole_value_locals_are_passed_as_they_are() {
    let modes = vec![ArgMode::Out, ArgMode::Borrowed, ArgMode::Borrowed];
    assert!(needing_copy(call(vec![copy(2), copy(1), copy(1)], modes, place(0))).is_empty());
}

#[test]
fn a_copied_value_argument_needs_no_further_copy() {
    let modes = vec![ArgMode::Out, ArgMode::Borrowed];
    let mut body = caller(call(vec![copy(1), copy(1)], modes, place(0)));
    copy_value_arguments(&mut body);
    assert!(value_arguments_needing_copy(&body, 0).is_empty());
    let Some(Terminator {
        kind: TerminatorKind::Call { args, .. },
        ..
    }) = &body.basic_blocks[0].terminator
    else {
        panic!("the call is gone");
    };
    assert_eq!(args.as_slice(), &[copy(1), copy(4)]);
    assert_eq!(
        body.basic_blocks[0].statements[0].kind,
        StatementKind::Assign(place(4), Rvalue::Use(copy(1)))
    );
}

fn params(locals: &[usize]) -> HashSet<Local> {
    locals.iter().copied().map(Local).collect()
}

/// A callee over [`value_locals`] taking `_1` and `_2`, that runs `entry`.
fn callee(entry: miri::mir::BasicBlockData) -> miri::mir::Body {
    body_over(&value_locals(), 2, entry)
}

#[test]
fn a_body_that_only_reads_its_value_parameters_writes_none() {
    let body = callee(block(
        vec![assign(field(0, 0), Rvalue::Use(Operand::Copy(field(1, 1))))],
        call(vec![copy(1), copy(2)], Vec::new(), place(0)),
    ));
    assert!(written_value_params(&body).is_empty());
}

#[test]
fn a_value_parameter_assigned_through_a_field_is_written() {
    let body = callee(block(
        vec![assign(field(2, 0), Rvalue::Use(Operand::Copy(field(3, 1))))],
        goto_exit(),
    ));
    assert_eq!(written_value_params(&body), params(&[2]));
}

#[test]
fn a_value_parameter_receiving_a_call_result_is_written() {
    let body = callee(block(Vec::new(), call(vec![copy(2)], Vec::new(), place(1))));
    assert_eq!(written_value_params(&body), params(&[1]));
}

#[test]
fn a_value_parameter_passed_as_out_is_written() {
    let modes = vec![ArgMode::Borrowed, ArgMode::Out];
    let body = callee(block(
        Vec::new(),
        call(vec![copy(1), copy(2)], modes, place(0)),
    ));
    assert_eq!(written_value_params(&body), params(&[2]));
}

#[test]
fn a_value_parameter_whose_address_is_taken_is_written() {
    let body = callee(block(
        vec![assign(place(0), Rvalue::Ref(place(1)))],
        goto_exit(),
    ));
    assert_eq!(written_value_params(&body), params(&[1]));
}

#[test]
fn an_out_parameter_and_a_word_parameter_are_never_counted() {
    let kinds = [pair(), pair(), pair(), TypeKind::Int];
    let entry = block(
        vec![
            assign(place(1), Rvalue::Use(copy(2))),
            assign(place(3), Rvalue::Use(copy(3))),
        ],
        goto_exit(),
    );
    let mut body = body_over(&kinds, 3, entry);
    body.out_params = vec![true, false, false];
    assert!(written_value_params(&body).is_empty());
}
