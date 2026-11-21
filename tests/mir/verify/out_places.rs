// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rules of taking a place for an `out` argument.
//!
//! The fixture is a call taking one element of a list `out`, the way a call
//! writing that element in place is lowered. Each test either checks a shape
//! lowering writes and finds nothing, or breaks that shape in one way and
//! finds exactly the one rule it breaks.

use miri::ast::type_arguments::{TypeArgKind, TypeArgument};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::place::PlaceElem;
use miri::mir::symbol::Symbol;
use miri::mir::verify::{verify_out_places, VerificationViolation};
use miri::mir::{
    ArgMode, Body, CallFlags, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use miri::pipeline::Pipeline;
use miri::type_checker::utils::ResourceTypes;

/// The list of strings whose element is taken.
const STRINGS: Local = Local(1);
/// The index of the element taken.
const INDEX: Local = Local(2);
/// The call's destination.
const RESULT: Local = Local(3);
/// A list of ints, whose elements are scalars.
const INTS: Local = Local(4);
/// A string beside the taken element.
const OTHER: Local = Local(5);
/// A second index into the list of strings.
const SECOND_INDEX: Local = Local(6);
/// A temp holding a copy of a value the call lends.
const HELD: Local = Local(7);

fn span() -> Span {
    Span::new(0, 0)
}

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

fn list_of(element: TypeKind) -> TypeKind {
    let argument = TypeArgument::new(0, TypeArgKind::Type(Box::new(ty(element)), false), span());
    TypeKind::List(Box::new(argument))
}

fn element(list: Local, index: Local) -> Place {
    Place {
        local: list,
        projection: vec![PlaceElem::index(index)],
    }
}

fn whole(local: Local) -> Place {
    Place::new(local)
}

/// A body running on `model` with the locals the constants above name.
fn body_on(model: ExecutionModel) -> Body {
    let mut body = Body::new(0, span(), model);
    for kind in [
        TypeKind::Void,
        list_of(TypeKind::String),
        TypeKind::Int,
        TypeKind::String,
        list_of(TypeKind::Int),
        TypeKind::String,
        TypeKind::Int,
        list_of(TypeKind::String),
    ] {
        body.new_local(LocalDecl::new(ty(kind), span()));
    }
    body
}

fn block(kind: TerminatorKind) -> BasicBlockData {
    BasicBlockData {
        statements: Vec::new(),
        terminator: Some(Terminator { kind, span: span() }),
        is_cleanup: false,
    }
}

/// `destination = f(args)` on `model`, each argument passed by its mode.
fn calling_on(
    model: ExecutionModel,
    func: Operand,
    args: Vec<(Operand, ArgMode)>,
    destination: Place,
) -> Body {
    let mut body = body_on(model);
    let (args, arg_modes) = args.into_iter().unzip();
    let call = TerminatorKind::Call {
        func,
        args,
        arg_modes,
        arg_handles: Vec::new(),
        flags: CallFlags::default(),
        destination,
        target: Some(BasicBlock(1)),
    };
    body.basic_blocks = vec![block(call), block(TerminatorKind::Return)];
    body
}

fn function() -> Operand {
    Operand::function(Symbol::entry(), span())
}

/// `result = f(args)` on the CPU.
fn calling(args: Vec<(Operand, ArgMode)>) -> Body {
    calling_on(ExecutionModel::Cpu, function(), args, whole(RESULT))
}

fn out(place: Place) -> (Operand, ArgMode) {
    (Operand::Copy(place), ArgMode::Out)
}

fn lent(place: Place) -> (Operand, ArgMode) {
    (Operand::Copy(place), ArgMode::Borrowed)
}

/// The one violation `body` raises, which must name `local` and say `words`.
fn assert_one(body: &Body, local: Local, words: &str) {
    let found: Vec<VerificationViolation> = verify_out_places(body, &ResourceTypes::default());
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].local, local, "{found:?}");
    assert!(found[0].message.contains(words), "{}", found[0].message);
}

fn assert_clean(body: &Body) {
    let found = verify_out_places(body, &ResourceTypes::default());
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn an_element_of_a_counted_type_taken_out_beside_another_value_is_clean() {
    assert_clean(&calling(vec![
        out(element(STRINGS, INDEX)),
        lent(whole(OTHER)),
    ]));
}

#[test]
fn a_whole_scalar_local_taken_out_is_clean() {
    assert_clean(&calling(vec![out(whole(INDEX))]));
}

#[test]
fn two_out_arguments_at_different_roots_are_clean() {
    assert_clean(&calling(vec![
        out(element(STRINGS, INDEX)),
        out(whole(OTHER)),
    ]));
}

#[test]
fn an_out_argument_moved_rather_than_copied_is_flagged() {
    let moved = (Operand::Move(element(STRINGS, INDEX)), ArgMode::Out);
    assert_one(&calling(vec![moved]), STRINGS, "not a place");
}

#[test]
fn an_out_argument_through_a_dereference_is_flagged() {
    let through = Place {
        local: OTHER,
        projection: vec![PlaceElem::Deref],
    };
    assert_one(&calling(vec![out(through)]), OTHER, "dereference");
}

#[test]
fn an_element_of_a_scalar_type_taken_by_its_address_is_flagged() {
    assert_one(
        &calling(vec![out(element(INTS, INDEX))]),
        INTS,
        "copied into a slot of its own",
    );
}

#[test]
fn a_place_taken_out_in_a_device_body_is_flagged() {
    let body = calling_on(
        ExecutionModel::GpuDevice,
        function(),
        vec![out(element(STRINGS, INDEX))],
        whole(RESULT),
    );
    assert_one(&body, STRINGS, "device");
}

#[test]
fn a_whole_local_taken_out_in_a_device_body_is_clean() {
    let body = calling_on(
        ExecutionModel::GpuDevice,
        function(),
        vec![out(whole(INDEX))],
        whole(RESULT),
    );
    assert_clean(&body);
}

#[test]
fn two_out_arguments_at_one_root_are_flagged() {
    assert_one(
        &calling(vec![
            out(element(STRINGS, INDEX)),
            out(element(STRINGS, SECOND_INDEX)),
        ]),
        STRINGS,
        "taken twice",
    );
}

#[test]
fn an_out_argument_at_the_root_the_destination_writes_is_flagged() {
    let body = calling_on(
        ExecutionModel::Cpu,
        function(),
        vec![out(element(STRINGS, INDEX))],
        element(STRINGS, SECOND_INDEX),
    );
    assert_one(&body, STRINGS, "destination");
}

#[test]
fn a_sibling_reading_the_taken_root_is_flagged() {
    assert_one(
        &calling(vec![
            out(element(STRINGS, INDEX)),
            lent(element(STRINGS, SECOND_INDEX)),
        ]),
        STRINGS,
        "while it is taken",
    );
}

#[test]
fn a_callee_read_from_the_taken_root_is_flagged() {
    let body = calling_on(
        ExecutionModel::Cpu,
        Operand::Copy(element(STRINGS, SECOND_INDEX)),
        vec![out(element(STRINGS, INDEX))],
        whole(RESULT),
    );
    assert_one(&body, STRINGS, "while it is taken");
}

#[test]
fn an_index_of_the_taken_place_written_by_the_call_is_flagged() {
    assert_one(
        &calling(vec![out(element(STRINGS, INDEX)), out(whole(INDEX))]),
        INDEX,
        "index",
    );
}

#[test]
fn an_index_of_the_taken_place_the_destination_writes_is_flagged() {
    let body = calling_on(
        ExecutionModel::Cpu,
        function(),
        vec![out(element(STRINGS, INDEX))],
        whole(INDEX),
    );
    assert_one(&body, INDEX, "index");
}

#[test]
fn a_virtual_call_taking_one_root_twice_is_flagged() {
    let mut body = body_on(ExecutionModel::Cpu);
    let call = TerminatorKind::VirtualCall {
        vtable_slot: 0,
        args: vec![
            Operand::Copy(whole(OTHER)),
            Operand::Copy(element(STRINGS, INDEX)),
            Operand::Copy(element(STRINGS, SECOND_INDEX)),
        ],
        arg_modes: vec![ArgMode::Borrowed, ArgMode::Out, ArgMode::Out],
        param_types: Vec::new(),
        destination: whole(RESULT),
        target: Some(BasicBlock(1)),
    };
    body.basic_blocks = vec![block(call), block(TerminatorKind::Return)];
    assert_one(&body, STRINGS, "taken twice");
}

/// `body` with `HELD = copy from` put first in the block its call ends.
fn holding(mut body: Body, from: Place) -> Body {
    let copy = Statement {
        kind: StatementKind::Assign(whole(HELD), Rvalue::Use(Operand::Copy(from))),
        span: span(),
    };
    body.basic_blocks[0].statements.push(copy);
    body
}

#[test]
fn a_temp_copied_from_the_list_holding_the_taken_element_is_flagged() {
    let body = holding(
        calling(vec![out(element(STRINGS, INDEX)), lent(whole(HELD))]),
        whole(STRINGS),
    );
    assert_one(&body, HELD, "lends a value holding the place");
}

#[test]
fn a_temp_copied_from_another_element_beside_the_taken_one_is_clean() {
    let body = holding(
        calling(vec![out(element(STRINGS, INDEX)), lent(whole(HELD))]),
        element(STRINGS, SECOND_INDEX),
    );
    assert_clean(&body);
}

#[test]
fn a_field_of_a_value_whose_type_cannot_be_derived_is_flagged() {
    let place = Place {
        local: OTHER,
        projection: vec![PlaceElem::Field(3)],
    };
    assert_one(&calling(vec![out(place)]), OTHER, "cannot be derived");
}

#[test]
fn an_out_constant_names_the_call_by_its_destination() {
    let constant = (Operand::function(Symbol::entry(), span()), ArgMode::Out);
    assert_one(&calling(vec![constant]), RESULT, "is not a place");
}

const SHARED_HOLDER: &str = "
use system.io
use system.collections.list

class Tagged
    var tags List<String>

fn retag(c out List<String>, h Tagged)
    c = List([\"new\"])

fn main()
    var h = Tagged(tags: List([\"old\"]))
    var k = h
    retag(h.tags, k)
    println(h.tags[0])
";

/// The violations `verify_out_places` finds in `main` of `SHARED_HOLDER`
/// once `mutate` has changed it.
fn shared_holder_violations(mutate: impl FnOnce(&mut Body)) -> Vec<VerificationViolation> {
    let (mut bodies, _) = Pipeline::new()
        .get_mir_program_with_rc(SHARED_HOLDER)
        .expect("the program lowers");
    let (_, main) = (bodies.iter_mut())
        .find(|(name, _)| name == "main")
        .expect("the program has a main");
    mutate(main);
    let result = Pipeline::new()
        .frontend_script(SHARED_HOLDER)
        .expect("the program checks");
    let resources = ResourceTypes::of(
        result.type_checker.type_definitions(),
        result.type_checker.ownership_instantiations(),
    );
    verify_out_places(main, &resources)
}

/// Hands the call taking a temp copied from a field `out` that field's
/// own place instead, as if it were taken by its address.
fn take_the_field_by_its_address(main: &mut Body) {
    for block in &mut main.basic_blocks {
        let copies: Vec<(Local, Place)> = (block.statements.iter())
            .filter_map(|statement| match &statement.kind {
                StatementKind::Assign(to, Rvalue::Use(Operand::Copy(from)))
                    if to.projection.is_empty() && !from.projection.is_empty() =>
                {
                    Some((to.local, from.clone()))
                }
                _ => None,
            })
            .collect();
        let Some(Terminator {
            kind: TerminatorKind::Call {
                args, arg_modes, ..
            },
            ..
        }) = &mut block.terminator
        else {
            continue;
        };
        for (arg, mode) in args.iter_mut().zip(arg_modes.iter()) {
            let Operand::Copy(place) = arg else { continue };
            let copied = copies.iter().find(|(temp, _)| *temp == place.local);
            if let (ArgMode::Out, Some((_, from))) = (mode, copied) {
                *arg = Operand::Copy(from.clone());
            }
        }
    }
}

#[test]
fn a_field_of_a_shared_class_instance_copied_in_and_out_is_clean() {
    let found = shared_holder_violations(|_| {});
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_field_of_a_shared_class_instance_taken_by_its_address_is_flagged() {
    let found = shared_holder_violations(take_the_field_by_its_address);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0]
            .message
            .contains("a class instance another binding may share"),
        "{}",
        found[0].message
    );
}
