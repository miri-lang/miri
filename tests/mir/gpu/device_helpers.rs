// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The GPU copy of a function a kernel calls: cloned from the body lowered
//! for the host, it hands every `out` argument over the way device code does,
//! copied into a temp of its own and stored back after the call.

use crate::mir::utils::{lower_named, out_handovers, runtime_callee_stating};
use miri::error::lowering::LoweringError;
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::call_graph::direct_callees;
use miri::mir::gpu::clone_gpu_device_helpers;
use miri::mir::gpu::device_helpers::copy_out_arguments_in;
use miri::mir::gpu_names::GpuNames;
use miri::mir::symbol::Symbol;
use miri::mir::{
    ArgMode, Body, CallFlags, ExecutionModel, Operand, Place, Statement, StatementKind, Terminator,
    TerminatorKind,
};
use miri::runtime_effects::ParamUse;

const HELPER: &str = "
use system.collections.list

struct Point
    x int
    y int

struct Segment
    a Point
    b Point

fn shift(p out Point)
    p.x = p.x + 1

fn bump(n out int)
    n = n + 1

fn grow(xs out List<int>)
    xs.push(1)

fn helper(k int) int
    var s = Segment(a: Point(x: 1, y: 2), b: Point(x: 3, y: 4))
    var n = k
    var i = 0
    var all = List([List([1])])
    shift(s.a)
    bump(n)
    grow(all[i])
    s.a.x + n

fn main()
    let r = helper(1)
";

/// How many `out` operands of `body` are handed by a projected place.
fn projected_operands(body: &Body) -> usize {
    (out_handovers(body).iter())
        .filter(|handover| !handover.operand.projection.is_empty())
        .count()
}

/// Asserts every `out` argument of `body` is a whole temp copied from a place
/// and stored back into it, `projected` of those places being a field or an
/// element.
fn assert_copied_in(body: &Body, projected: usize) {
    let found = out_handovers(body);
    assert_eq!(found.len(), 3, "three out arguments:\n{body}");
    for handover in &found {
        assert!(
            handover.operand.projection.is_empty(),
            "an out operand in device code is a whole temp, not `{}`:\n{body}",
            handover.operand
        );
        assert!(handover.copied_from.is_some(), "no copy-in:\n{body}");
        assert_eq!(
            handover.copied_from, handover.stored_back_into,
            "stored back into the place it was copied from:\n{body}"
        );
    }
    let places = (found.iter())
        .filter_map(|handover| handover.copied_from.as_ref())
        .filter(|place| !place.projection.is_empty())
        .count();
    assert_eq!(places, projected, "{body}");
}

#[test]
fn a_host_body_hands_a_held_or_counted_slot_by_its_place() {
    let body = lower_named(HELPER, "helper", false);
    assert_eq!(projected_operands(&body), 2, "`s.a` and `all[i]`:\n{body}");
}

#[test]
fn a_device_copy_hands_every_out_argument_copied_in() {
    let mut body = lower_named(HELPER, "helper", false);
    body.execution_model = ExecutionModel::GpuDevice;
    copy_out_arguments_in(&mut body).expect("rewrite");
    assert_copied_in(&body, 2);
}

#[test]
fn a_host_body_keeps_its_out_arguments_by_address() {
    let mut body = lower_named(HELPER, "helper", false);
    let before = body.clone();
    copy_out_arguments_in(&mut body).expect("rewrite");
    assert_eq!(body, before, "a host body hands its slots over as lowered");
}

/// The bodies of `HELPER`'s `main` lowered as a kernel and of `helper` as
/// the host lowers it, the kernel's call to `helper` linking the two, after
/// the helper is cloned for device code; `amend` is handed the host helper
/// first.
fn clone_for_a_kernel(amend: impl FnOnce(&mut Body)) -> Result<Vec<(Symbol, Body)>, LoweringError> {
    let mut helper = lower_named(HELPER, "helper", false);
    amend(&mut helper);
    let mut kernel = lower_named(HELPER, "main", true);
    kernel.execution_model = ExecutionModel::GpuKernel;
    let helper_symbol = direct_callees(&kernel)
        .next()
        .cloned()
        .expect("main calls helper");
    let mut bodies: Vec<(Symbol, Body)> = vec![(Symbol::entry(), kernel), (helper_symbol, helper)];
    let mut names = GpuNames::collect(&bodies).expect("names");
    clone_gpu_device_helpers(&mut bodies, &mut names)?;
    Ok(bodies)
}

/// A call asking whether `_1` is shared, continuing in a block that does
/// more than branch on the answer, appended to `body` where nothing reaches
/// it.
fn append_an_unbranched_count_question(body: &mut Body) {
    let span = Span::new(0, 0);
    let asks = runtime_callee_stating("is_shared_for_test", vec![ParamUse::CountOf]);
    let after = BasicBlock(body.basic_blocks.len() + 1);
    let question = Terminator::new(
        TerminatorKind::Call {
            func: Operand::function(Symbol::runtime(asks), span),
            args: vec![Operand::Copy(Place::new(miri::mir::Local(1)))],
            arg_modes: vec![ArgMode::Borrowed],
            arg_handles: Vec::new(),
            flags: CallFlags::default(),
            destination: Place::new(miri::mir::Local(0)),
            target: Some(after),
        },
        span,
    );
    let mut continued = BasicBlockData::new(Some(Terminator::new(TerminatorKind::Return, span)));
    continued.statements.push(Statement {
        kind: StatementKind::Nop,
        span,
    });
    body.basic_blocks.push(BasicBlockData::new(Some(question)));
    body.basic_blocks.push(continued);
    body.cfg_changed();
}

#[test]
fn a_count_question_device_code_cannot_answer_is_an_internal_error() {
    let error = clone_for_a_kernel(append_an_unbranched_count_question)
        .err()
        .expect("an unanswered count question fails the clone");
    let text = format!("{error:?}");
    assert!(text.contains("internal compiler error"), "{text}");
    assert!(text.contains("is_shared_for_test"), "{text}");
}

#[test]
fn cloning_a_helper_for_a_kernel_copies_its_out_arguments_in() {
    let bodies = clone_for_a_kernel(|_| {}).expect("clone");
    let device = (bodies.iter())
        .find(|(_, body)| body.execution_model == ExecutionModel::GpuDevice)
        .map(|(_, body)| body)
        .expect("a device copy of helper");
    assert_copied_in(device, 2);
    let host = (bodies.iter())
        .find(|(_, body)| body.execution_model == ExecutionModel::Cpu)
        .map(|(_, body)| body)
        .expect("the host helper");
    assert_eq!(
        projected_operands(host),
        2,
        "the host body is left as it is"
    );
}
