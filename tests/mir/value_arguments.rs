// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for which value arguments a call needs copied, and for which value
//! parameters a body writes and so copies on entry.

use super::value_bodies::*;
use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::TypeKind;
use miri::mir::place::PlaceElem;
use miri::mir::symbol::Symbol;
use miri::mir::value_arguments::{
    copy_value_arguments, exposure_of, value_arguments_needing_copy, written_value_params, Exposure,
};
use miri::mir::{
    ArgMode, BasicBlock, Constant, Local, Operand, Place, Rvalue, StatementKind, Terminator,
    TerminatorKind,
};
use miri::runtime_fns::rt;
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

/// The position and reason of every argument of `kind`'s call that needs a
/// copy, made by a caller over [`holder_locals`].
fn holder_needing_copy(kind: TerminatorKind) -> Vec<(usize, Exposure)> {
    let body = holder_caller(kind);
    value_arguments_needing_copy(&body, 0)
        .into_iter()
        .map(|argument| (argument.position, argument.exposure))
        .collect()
}

#[test]
fn a_field_of_a_value_local_is_passed_in_place() {
    let args = vec![Operand::Copy(field(3, 0))];
    assert!(holder_needing_copy(call(args, Vec::new(), place(0))).is_empty());
}

#[test]
fn the_copy_pass_leaves_a_call_on_a_field_of_a_value_local_as_it_is() {
    let args = vec![Operand::Copy(field(3, 0)), copy(3)];
    let mut body = holder_caller(call(args, Vec::new(), place(0)));
    let before = body.clone();
    copy_value_arguments(&mut body);
    assert_eq!(body.local_decls.len(), before.local_decls.len());
    assert_eq!(body.basic_blocks, before.basic_blocks);
}

#[test]
fn a_nested_field_of_a_value_local_is_passed_in_place() {
    let inner = projected(7, vec![PlaceElem::Field(0), PlaceElem::Field(0)]);
    let args = vec![Operand::Copy(inner)];
    assert!(holder_needing_copy(call(args, Vec::new(), place(0))).is_empty());
}

#[test]
fn a_field_of_a_counted_holder_is_copied() {
    let args = vec![Operand::Copy(field(4, 0))];
    assert_eq!(
        holder_needing_copy(call(args, Vec::new(), place(0))),
        vec![(0, Exposure::Projected)]
    );
}

#[test]
fn a_member_of_a_value_picked_by_a_runtime_index_is_copied() {
    let picked = projected(5, vec![PlaceElem::Index(Local(6))]);
    let args = vec![Operand::Copy(picked)];
    assert_eq!(
        holder_needing_copy(call(args, Vec::new(), place(0))),
        vec![(0, Exposure::Projected)]
    );
}

#[test]
fn a_field_of_the_local_the_call_writes_as_its_destination_is_copied() {
    let args = vec![Operand::Copy(field(3, 0))];
    assert_eq!(
        holder_needing_copy(call(args, Vec::new(), field(3, 0))),
        vec![(0, Exposure::Destination)]
    );
}

#[test]
fn a_field_of_the_local_the_call_writes_through_out_is_copied() {
    let modes = vec![ArgMode::Borrowed, ArgMode::Out];
    let args = vec![Operand::Copy(field(3, 0)), copy(3)];
    assert_eq!(
        holder_needing_copy(call(args, modes, place(0))),
        vec![(0, Exposure::Out)]
    );
}

#[test]
fn a_field_beside_its_whole_holder_is_passed_in_place() {
    let args = vec![Operand::Copy(field(3, 0)), copy(3)];
    assert!(holder_needing_copy(call(args, Vec::new(), place(0))).is_empty());
}

#[test]
fn exposure_of_names_why_each_place_needs_a_copy() {
    let body = holder_caller(goto_exit());
    let destination = field(1, 0);
    let written = [Local(1), Local(2)];
    let exposure = |place: &Place| exposure_of(&body, place, &destination, &written);
    assert_eq!(exposure(&place(3)), None);
    assert_eq!(exposure(&field(3, 0)), None);
    assert_eq!(exposure(&place(1)), Some(Exposure::Destination));
    assert_eq!(exposure(&place(2)), Some(Exposure::Out));
    assert_eq!(exposure(&field(2, 0)), Some(Exposure::Out));
    assert_eq!(exposure(&field(4, 0)), Some(Exposure::Projected));
    let picked = projected(5, vec![PlaceElem::Index(Local(6))]);
    assert_eq!(exposure(&picked), Some(Exposure::Projected));
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

/// A call to the runtime entry point `name` taking `args`, going on to
/// block 1.
fn runtime_call(name: &str, args: Vec<Operand>, arg_modes: Vec<ArgMode>) -> TerminatorKind {
    TerminatorKind::Call {
        func: Operand::function(Symbol::runtime(name), span()),
        args,
        arg_modes,
        arg_handles: Vec::new(),
        destination: place(0),
        target: Some(BasicBlock(1)),
    }
}

/// The byte count the element ABI passes beside an element's address.
fn byte_count() -> Operand {
    Operand::Constant(Box::new(Constant {
        span: span(),
        ty: ty(TypeKind::Int),
        literal: Literal::Integer(IntegerLiteral::I64(16)),
    }))
}

/// A callee taking `_1` and `_2`, whose `_4` is an address and `_5` a list,
/// that takes the address of `_1` into `_4` and then makes `kind`.
fn addressing_callee(kind: TerminatorKind) -> miri::mir::Body {
    let mut kinds = value_locals();
    kinds.push(TypeKind::RawPtr);
    kinds.push(TypeKind::List(Box::new(type_expr(TypeKind::Int))));
    body_over(
        &kinds,
        2,
        block(vec![assign(place(4), Rvalue::Ref(place(1)))], kind),
    )
}

#[test]
fn a_value_parameter_pushed_by_address_is_not_written() {
    let push = runtime_call(
        rt::LIST_PUSH,
        vec![copy(5), copy(4), byte_count()],
        Vec::new(),
    );
    assert!(written_value_params(&addressing_callee(push)).is_empty());
}

#[test]
fn a_value_parameter_stored_as_a_map_value_by_address_is_not_written() {
    let args = vec![copy(5), copy(3), byte_count(), copy(4), byte_count()];
    let set = runtime_call(rt::MAP_SET, args, Vec::new());
    assert!(written_value_params(&addressing_callee(set)).is_empty());
}

#[test]
fn a_value_parameter_whose_address_a_runtime_call_writes_into_is_written() {
    let modes = vec![ArgMode::Borrowed, ArgMode::Borrowed, ArgMode::Borrowed];
    let get = runtime_call(
        rt::MAP_GET,
        vec![copy(5), copy(2), byte_count(), copy(4)],
        modes,
    );
    assert_eq!(written_value_params(&addressing_callee(get)), params(&[1]));
}

#[test]
fn a_value_parameter_whose_element_address_is_also_used_elsewhere_is_written() {
    let push = runtime_call(
        rt::LIST_PUSH,
        vec![copy(5), copy(4), byte_count()],
        Vec::new(),
    );
    let mut body = addressing_callee(push);
    body.basic_blocks[1]
        .statements
        .push(assign(place(4), Rvalue::Use(copy(4))));
    assert_eq!(written_value_params(&body), params(&[1]));
}
