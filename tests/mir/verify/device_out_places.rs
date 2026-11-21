// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rules of handing an `out` argument over in device code.
//!
//! The fixture is a kernel writing one element of a storage buffer through a
//! call, the way device code lowers it: the element is copied into a temp of
//! its own, the temp is handed over, and the temp is stored back into the
//! element first thing after the call. Each test breaks that shape in one way
//! and finds exactly the one rule it breaks, or checks a shape lowering writes
//! and finds nothing.

use crate::mir::utils::{lower_named, runtime_callee_stating};
use miri::ast::type_arguments::{TypeArgKind, TypeArgument};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::gpu::device_helpers::copy_out_arguments_in;
use miri::mir::place::PlaceElem;
use miri::mir::symbol::Symbol;
use miri::mir::verify::{verify_out_places, VerificationViolation};
use miri::mir::{
    ArgMode, Body, CallFlags, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind, StorageClass, Terminator, TerminatorKind,
};
use miri::runtime_effects::ParamUse;
use miri::type_checker::utils::ResourceTypes;

/// The storage buffer the kernel writes, its first parameter.
const BUFFER: Local = Local(1);
/// The thread's index into the buffer, its second parameter.
const THREAD: Local = Local(2);
/// The temp the element is copied into for the call.
const COPY: Local = Local(3);
/// The call's destination.
const RESULT: Local = Local(4);

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

fn element() -> Place {
    Place {
        local: BUFFER,
        projection: vec![PlaceElem::index(THREAD)],
    }
}

fn copy(to: Place, from: Place) -> Statement {
    Statement {
        kind: StatementKind::Assign(to, Rvalue::Use(Operand::Copy(from))),
        span: span(),
    }
}

fn block(statements: Vec<Statement>, kind: TerminatorKind) -> BasicBlockData {
    BasicBlockData {
        statements,
        terminator: Some(Terminator { kind, span: span() }),
        is_cleanup: false,
    }
}

fn call(func: Operand, args: Vec<(Operand, ArgMode)>) -> TerminatorKind {
    let (args, arg_modes) = args.into_iter().unzip();
    TerminatorKind::Call {
        func,
        args,
        arg_modes,
        arg_handles: Vec::new(),
        flags: CallFlags::default(),
        destination: Place::new(RESULT),
        target: Some(BasicBlock(1)),
    }
}

fn function() -> Operand {
    Operand::function(Symbol::entry(), span())
}

/// A body on `model` over a buffer parameter and an index parameter, with no
/// blocks yet.
fn kernel_locals(model: ExecutionModel) -> Body {
    let mut body = Body::new(2, span(), model);
    for kind in [
        TypeKind::Void,
        list_of(TypeKind::Int),
        TypeKind::Int,
        TypeKind::Int,
        TypeKind::Void,
    ] {
        body.new_local(LocalDecl::new(ty(kind), span()));
    }
    body.local_decls[BUFFER.0].storage_class = StorageClass::StorageBuffer;
    body.signature.out_params = vec![false, false];
    body.signature.storage_read_write = vec![true, false];
    body.signature.param_written = vec![true, false];
    body
}

/// The kernel as device code lowers it:
/// `COPY = BUFFER[THREAD]; f(out COPY); BUFFER[THREAD] = COPY`.
fn kernel() -> Body {
    let mut body = kernel_locals(ExecutionModel::GpuKernel);
    body.basic_blocks = vec![
        block(
            vec![copy(Place::new(COPY), element())],
            call(
                function(),
                vec![(Operand::Copy(Place::new(COPY)), ArgMode::Out)],
            ),
        ),
        block(
            vec![copy(element(), Place::new(COPY))],
            TerminatorKind::Return,
        ),
    ];
    body
}

fn found(body: &Body) -> Vec<VerificationViolation> {
    verify_out_places(body, &ResourceTypes::default())
}

/// The one violation `body` raises, which must name `local` and say `words`.
fn assert_one(body: &Body, local: Local, words: &str) {
    let found = found(body);
    assert_eq!(found.len(), 1, "{found:?}\n{body}");
    assert_eq!(found[0].local, local, "{found:?}");
    assert!(found[0].message.contains(words), "{}", found[0].message);
}

fn assert_clean(body: &Body) {
    let found = found(body);
    assert!(found.is_empty(), "{found:?}\n{body}");
}

/// The out argument of the call ending the first block of `body`.
fn out_argument(body: &mut Body) -> &mut Operand {
    match body.basic_blocks[0]
        .terminator
        .as_mut()
        .map(|t| &mut t.kind)
    {
        Some(TerminatorKind::Call { args, .. }) => &mut args[0],
        other => panic!("the fixture ends its first block in a call, not {other:?}"),
    }
}

#[test]
fn a_kernel_copying_an_element_in_and_storing_it_back_is_clean() {
    assert_clean(&kernel());
}

#[test]
fn a_projected_out_argument_in_device_code_is_flagged() {
    let mut body = kernel();
    *out_argument(&mut body) = Operand::Copy(element());
    body.basic_blocks[0].statements.clear();
    body.basic_blocks[1].statements.clear();
    assert_one(&body, BUFFER, "whole temp");
}

#[test]
fn an_out_parameter_handed_on_whole_in_device_code_is_flagged() {
    let mut body = kernel_locals(ExecutionModel::GpuDevice);
    body.local_decls[BUFFER.0].storage_class = StorageClass::Stack;
    body.local_decls[THREAD.0].ty = ty(TypeKind::Int);
    body.signature.out_params = vec![false, true];
    body.signature.storage_read_write = Vec::new();
    body.basic_blocks = vec![
        block(
            Vec::new(),
            call(
                function(),
                vec![(Operand::Copy(Place::new(THREAD)), ArgMode::Out)],
            ),
        ),
        block(Vec::new(), TerminatorKind::Return),
    ];
    assert_one(&body, THREAD, "whole temp");
}

#[test]
fn an_out_temp_copied_from_no_place_is_flagged() {
    let mut body = kernel();
    body.basic_blocks[0].statements.clear();
    assert_one(&body, COPY, "copied from");
}

#[test]
fn a_dropped_store_back_is_flagged() {
    let mut body = kernel();
    body.basic_blocks[1].statements.clear();
    assert_one(&body, COPY, "stored back");
}

#[test]
fn a_store_back_into_another_place_is_flagged() {
    let mut body = kernel();
    body.basic_blocks[1].statements = vec![copy(Place::new(RESULT), Place::new(COPY))];
    assert_one(&body, COPY, "stored back");
}

#[test]
fn a_store_back_in_a_block_of_its_own_before_the_successor_is_clean() {
    let mut body = kernel();
    let stores = std::mem::take(&mut body.basic_blocks[1].statements);
    body.basic_blocks.push(block(
        stores,
        TerminatorKind::Goto {
            target: BasicBlock(1),
        },
    ));
    if let Some(TerminatorKind::Call { target, .. }) = body.basic_blocks[0]
        .terminator
        .as_mut()
        .map(|t| &mut t.kind)
    {
        *target = Some(BasicBlock(2));
    }
    assert_clean(&body);
}

#[test]
fn a_written_buffer_bound_read_only_is_flagged() {
    let mut body = kernel();
    body.signature.storage_read_write = vec![false, false];
    assert_one(&body, BUFFER, "read_write");
}

#[test]
fn a_kernel_out_parameter_bound_read_only_is_flagged() {
    let mut body = kernel();
    body.signature.out_params = vec![false, true];
    assert_one(&body, THREAD, "read_write");
}

#[test]
fn a_helper_out_parameter_of_buffer_type_is_flagged() {
    let mut body = kernel();
    body.execution_model = ExecutionModel::GpuDevice;
    body.local_decls[BUFFER.0].storage_class = StorageClass::Stack;
    body.signature.storage_read_write = Vec::new();
    body.signature.out_params = vec![true, false];
    assert_one(&body, BUFFER, "buffer");
}

#[test]
fn an_unshare_call_in_device_code_is_flagged() {
    let mut body = kernel();
    let cow = runtime_callee_stating("unshare_for_test", vec![ParamUse::CopiesOnWrite]);
    let unshare = call(
        Operand::function(Symbol::runtime(cow), span()),
        vec![(Operand::Copy(Place::new(BUFFER)), ArgMode::Borrowed)],
    );
    body.basic_blocks[1].terminator = Some(Terminator {
        kind: unshare,
        span: span(),
    });
    body.basic_blocks[1]
        .statements
        .push(copy(Place::new(COPY), Place::new(COPY)));
    body.basic_blocks
        .push(block(Vec::new(), TerminatorKind::Return));
    if let Some(TerminatorKind::Call { target, .. }) = body.basic_blocks[1]
        .terminator
        .as_mut()
        .map(|t| &mut t.kind)
    {
        *target = Some(BasicBlock(2));
    }
    assert_one(&body, BUFFER, "copy-on-write");
}

const HELPER: &str = "
struct Point
    x int
    y int

fn shift(p out Point)
    p.x = p.x + 1

fn bump(n out int)
    n = n + 1

fn helper(k int) int
    var s = Point(x: 1, y: 2)
    var n = k
    shift(s)
    bump(s.y)
    bump(n)
    s.x + n

fn main()
    let r = helper(1)
";

#[test]
fn a_helper_lowered_and_cloned_for_device_code_is_clean() {
    let mut body = lower_named(HELPER, "helper", false);
    body.execution_model = ExecutionModel::GpuDevice;
    copy_out_arguments_in(&mut body).expect("the rewrite succeeds");
    assert_clean(&body);
}

#[test]
fn a_helper_lowered_for_the_host_and_run_as_device_code_is_flagged() {
    let mut body = lower_named(HELPER, "helper", false);
    body.execution_model = ExecutionModel::GpuDevice;
    assert!(!found(&body).is_empty(), "{body}");
}

/// A binding of the kernel's, the index the second `out` argument's element
/// is reached through.
const INDEX: Local = Local(5);
/// The temp the kernel holds `INDEX` in, so a store back into `INDEX`
/// leaves the element's index as it was.
const HELD: Local = Local(6);
/// The temp the second `out` argument is copied into.
const SECOND: Local = Local(7);

fn element_at(index: Local) -> Place {
    Place {
        local: BUFFER,
        projection: vec![PlaceElem::index(index)],
    }
}

/// A kernel calling `f(out INDEX, out BUFFER[HELD])` the way device code
/// lowers it: `HELD = INDEX; COPY = INDEX; SECOND = BUFFER[HELD]`, the call,
/// then `INDEX = COPY; BUFFER[HELD] = SECOND`.
fn two_out_kernel() -> Body {
    let mut body = kernel_locals(ExecutionModel::GpuKernel);
    let mut index = LocalDecl::new(ty(TypeKind::Int), span());
    index.name = Some("j".into());
    index.is_user_variable = true;
    body.new_local(index);
    body.new_local(LocalDecl::new(ty(TypeKind::Int), span()));
    body.new_local(LocalDecl::new(ty(TypeKind::Int), span()));
    body.basic_blocks = vec![
        block(
            vec![
                copy(Place::new(HELD), Place::new(INDEX)),
                copy(Place::new(COPY), Place::new(INDEX)),
                copy(Place::new(SECOND), element_at(HELD)),
            ],
            call(
                function(),
                vec![
                    (Operand::Copy(Place::new(COPY)), ArgMode::Out),
                    (Operand::Copy(Place::new(SECOND)), ArgMode::Out),
                ],
            ),
        ),
        block(
            vec![
                copy(Place::new(INDEX), Place::new(COPY)),
                copy(element_at(HELD), Place::new(SECOND)),
            ],
            TerminatorKind::Return,
        ),
    ];
    body
}

/// Replaces every `HELD` the second `out` argument's element is reached
/// through, in its copy-in and its store-back, with `index`.
fn reach_the_element_through(body: &mut Body, index: Local) {
    body.basic_blocks[0].statements[2] = copy(Place::new(SECOND), element_at(index));
    body.basic_blocks[1].statements[1] = copy(element_at(index), Place::new(SECOND));
}

#[test]
fn a_kernel_handing_a_binding_and_an_element_at_its_held_index_is_clean() {
    assert_clean(&two_out_kernel());
}

#[test]
fn two_out_arguments_copied_from_one_buffer_are_flagged() {
    let mut body = two_out_kernel();
    body.basic_blocks[0].statements[1] = copy(Place::new(COPY), element());
    body.basic_blocks[1].statements[0] = copy(element(), Place::new(COPY));
    assert_one(&body, BUFFER, "taken twice");
}

#[test]
fn an_out_argument_copied_from_the_destination_is_flagged() {
    let mut body = two_out_kernel();
    if let Some(TerminatorKind::Call { destination, .. }) = body.basic_blocks[0]
        .terminator
        .as_mut()
        .map(|t| &mut t.kind)
    {
        *destination = Place::new(INDEX);
    }
    assert_one(&body, INDEX, "destination");
}

#[test]
fn an_index_restored_before_the_element_is_stored_back_is_flagged() {
    let mut body = two_out_kernel();
    body.basic_blocks[1]
        .statements
        .insert(0, copy(Place::new(HELD), Place::new(COPY)));
    assert_one(&body, HELD, "index");
}

#[test]
fn an_element_reached_through_another_out_argument_is_flagged() {
    let mut body = two_out_kernel();
    reach_the_element_through(&mut body, INDEX);
    assert_one(&body, INDEX, "index");
}

#[test]
fn an_element_reached_through_a_binding_the_call_leaves_alone_is_clean() {
    let mut body = two_out_kernel();
    reach_the_element_through(&mut body, THREAD);
    assert_clean(&body);
}

#[test]
fn a_kernel_storage_access_short_of_its_parameters_is_flagged() {
    let mut body = kernel();
    body.signature.storage_read_write = vec![true];
    assert_one(&body, Local(0), "storage access");
}

#[test]
fn an_atomic_buffer_bound_read_only_is_flagged() {
    let mut body = kernel();
    let atomic = TypeKind::Custom(
        "Atomic".into(),
        Some(vec![TypeArgument::new(
            0,
            TypeArgKind::Type(Box::new(ty(TypeKind::U32)), false),
            span(),
        )]),
    );
    body.local_decls[THREAD.0].ty = ty(list_of(atomic));
    body.local_decls[THREAD.0].storage_class = StorageClass::StorageBuffer;
    assert_one(&body, THREAD, "atomic");
}
