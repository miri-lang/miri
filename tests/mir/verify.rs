// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the path-sensitive reference-counting verifier.
//!
//! Each fixture is a hand-built MIR body rather than lowered Miri source, because
//! the defects being reproduced are ones lowering no longer emits: the point is to
//! prove the verifier still recognizes them, and a body built by hand pins the
//! exact statement placement that made each one a bug.
//!
//! Every seam appears twice — once broken, once corrected — so a test passing
//! because the verifier is right is distinguishable from one passing because the
//! verifier reports nothing at all.

use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::body::{BindingResidency, DeviceHandleId};
use miri::mir::verify::{
    verify_body, verify_collection_element_ownership, verify_cross_residency_readback,
    VerificationViolation,
};
use miri::mir::{
    AggregateKind, Body, Constant, Discriminant, ExecutionModel, GpuLaunchArgs, Local, LocalDecl,
    Operand, Place, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};
use std::collections::HashSet;

fn span() -> Span {
    Span::new(0, 0)
}

fn string_ty() -> Type {
    Type::new(TypeKind::String, span())
}

fn void_ty() -> Type {
    Type::new(TypeKind::Void, span())
}

fn place(local: usize) -> Place {
    Place::new(Local(local))
}

fn stmt(kind: StatementKind) -> Statement {
    Statement { kind, span: span() }
}

fn constant(ty: Type, literal: Literal) -> Operand {
    Operand::Constant(Box::new(Constant {
        ty,
        literal,
        span: span(),
    }))
}

/// An rvalue handing its destination a freshly allocated managed value.
fn fresh_string() -> Rvalue {
    Rvalue::Use(constant(string_ty(), Literal::String("x".to_string())))
}

fn storage_live(local: usize) -> Statement {
    stmt(StatementKind::StorageLive(place(local)))
}

fn storage_dead(local: usize) -> Statement {
    stmt(StatementKind::StorageDead(place(local)))
}

fn decref(local: usize) -> Statement {
    stmt(StatementKind::DecRef(place(local)))
}

fn dealloc(local: usize) -> Statement {
    stmt(StatementKind::Dealloc(place(local)))
}

fn assign_fresh(local: usize) -> Statement {
    stmt(StatementKind::Assign(place(local), fresh_string()))
}

fn incref(local: usize) -> Statement {
    stmt(StatementKind::IncRef(place(local)))
}

fn assign_copy(dest: usize, source: usize) -> Statement {
    stmt(StatementKind::Assign(
        place(dest),
        Rvalue::Use(Operand::Copy(place(source))),
    ))
}

fn terminator(kind: TerminatorKind) -> Terminator {
    Terminator { kind, span: span() }
}

fn goto(target: usize) -> Terminator {
    terminator(TerminatorKind::Goto {
        target: BasicBlock(target),
    })
}

fn branch(then_block: usize, else_block: usize) -> Terminator {
    terminator(TerminatorKind::SwitchInt {
        discr: constant(Type::new(TypeKind::Boolean, span()), Literal::Boolean(true)),
        targets: vec![(Discriminant::bool_true(), BasicBlock(then_block))],
        otherwise: BasicBlock(else_block),
    })
}

fn callee(return_ty: Type) -> Operand {
    constant(return_ty, Literal::String("callee".to_string()))
}

/// A call whose destination is unmanaged, so it moves no ownership of its own.
fn call_returning_void(arg: usize, destination: usize, target: usize) -> Terminator {
    terminator(TerminatorKind::Call {
        func: callee(void_ty()),
        args: vec![Operand::Copy(place(arg))],
        out_args: Vec::new(),
        arg_handles: Vec::new(),
        destination: place(destination),
        target: Some(BasicBlock(target)),
    })
}

/// A call handing back a freshly owned managed value.
fn call_returning_string(destination: usize, target: usize) -> Terminator {
    terminator(TerminatorKind::Call {
        func: callee(string_ty()),
        args: Vec::new(),
        out_args: Vec::new(),
        arg_handles: Vec::new(),
        destination: place(destination),
        target: Some(BasicBlock(target)),
    })
}

/// A call to a named runtime intrinsic, spelling the callee the way lowering does.
fn runtime_call(name: &str, args: Vec<Operand>, destination: usize, target: usize) -> Terminator {
    terminator(TerminatorKind::Call {
        func: constant(void_ty(), Literal::Identifier(name.to_string())),
        args,
        out_args: Vec::new(),
        arg_handles: Vec::new(),
        destination: place(destination),
        target: Some(BasicBlock(target)),
    })
}

fn ret() -> Terminator {
    terminator(TerminatorKind::Return)
}

fn block(statements: Vec<Statement>, terminator: Terminator) -> BasicBlockData {
    BasicBlockData {
        statements,
        terminator: Some(terminator),
        is_cleanup: false,
    }
}

/// A body whose local `i` has type `local_tys[i]`; local 0 is the return slot and
/// locals `1..=arg_count` are the parameters.
fn body_of(local_tys: &[Type], arg_count: usize, blocks: Vec<BasicBlockData>) -> Body {
    let mut body = Body::new(arg_count, span(), ExecutionModel::Cpu);
    for local_ty in local_tys {
        body.new_local(LocalDecl::new(local_ty.clone(), span()));
    }
    for block in blocks {
        body.basic_blocks.push(block);
    }
    body
}

/// Locals: 0 the return slot, 1 the managed temp, 2 an unmanaged call destination.
fn temp_body(blocks: Vec<BasicBlockData>) -> Body {
    body_of(&[void_ty(), string_ty(), void_ty()], 0, blocks)
}

fn messages(violations: &[VerificationViolation]) -> String {
    violations
        .iter()
        .map(|violation| violation.to_string())
        .collect::<Vec<_>>()
        .join("; ")
}

fn assert_clean(body: &Body, what: &str) {
    let violations = verify_body(body);
    assert!(
        violations.is_empty(),
        "expected no findings for {}, got: {}",
        what,
        messages(&violations)
    );
}

#[test]
fn clean_body_reports_nothing_and_a_finding_names_its_local() {
    let balanced = temp_body(vec![block(
        vec![storage_live(1), assign_fresh(1), decref(1), storage_dead(1)],
        ret(),
    )]);
    assert_clean(&balanced, "an acquire and release on one path");

    let leaking = temp_body(vec![block(
        vec![storage_live(1), assign_fresh(1), storage_dead(1)],
        ret(),
    )]);
    let violations = verify_body(&leaking);
    assert_eq!(violations.len(), 1, "got: {}", messages(&violations));
    let rendered = violations[0].to_string();
    assert!(
        rendered.starts_with("_1 (_1): "),
        "a finding reads as `local (name): message`, got {}",
        rendered
    );
}

/// A match arm ending in `return` skipped the scope pop that released the local,
/// leaving the release on the fall-through path and nowhere else.
#[test]
fn match_arm_returning_without_its_scope_pop_is_flagged() {
    let broken = temp_body(vec![
        block(vec![storage_live(1), assign_fresh(1)], branch(1, 2)),
        block(Vec::new(), ret()),
        block(vec![decref(1), storage_dead(1)], ret()),
    ]);

    let violations = verify_body(&broken);
    assert_eq!(violations.len(), 1, "got: {}", messages(&violations));
    assert_eq!(violations[0].local, Local(1));
    assert!(
        violations[0].message.contains("still owns 1 reference"),
        "got: {}",
        violations[0].message
    );
}

#[test]
fn match_arm_releasing_on_every_exit_is_clean() {
    let fixed = temp_body(vec![
        block(vec![storage_live(1), assign_fresh(1)], branch(1, 2)),
        block(vec![decref(1), storage_dead(1)], ret()),
        block(vec![decref(1), storage_dead(1)], ret()),
    ]);
    assert_clean(&fixed, "a match arm releasing on both exits");
}

/// The temporary holding a method call's argument was never released.
#[test]
fn method_call_argument_temp_left_unreleased_is_flagged() {
    let broken = temp_body(vec![
        block(
            vec![storage_live(1), assign_fresh(1)],
            call_returning_void(1, 2, 1),
        ),
        block(Vec::new(), ret()),
    ]);

    let violations = verify_body(&broken);
    assert_eq!(violations.len(), 1, "got: {}", messages(&violations));
    assert_eq!(violations[0].local, Local(1));
}

#[test]
fn method_call_argument_temp_released_after_the_call_is_clean() {
    let fixed = temp_body(vec![
        block(
            vec![storage_live(1), assign_fresh(1)],
            call_returning_void(1, 2, 1),
        ),
        block(vec![decref(1), storage_dead(1)], ret()),
    ]);
    assert_clean(&fixed, "an argument temp released after its call");
}

/// The value a coalescing operator produced was stranded on one of its branches.
#[test]
fn coalesce_result_stranded_on_one_branch_is_flagged() {
    let broken = temp_body(vec![
        block(vec![storage_live(1)], call_returning_string(1, 1)),
        block(Vec::new(), branch(2, 3)),
        block(vec![decref(1), storage_dead(1)], ret()),
        block(Vec::new(), ret()),
    ]);

    let violations = verify_body(&broken);
    assert_eq!(violations.len(), 1, "got: {}", messages(&violations));
    assert_eq!(violations[0].local, Local(1));
    assert!(
        violations[0].message.contains("still owns 1 reference"),
        "got: {}",
        violations[0].message
    );
}

#[test]
fn coalesce_result_released_on_both_branches_is_clean() {
    let fixed = temp_body(vec![
        block(vec![storage_live(1)], call_returning_string(1, 1)),
        block(Vec::new(), branch(2, 3)),
        block(vec![decref(1), storage_dead(1)], ret()),
        block(vec![decref(1), storage_dead(1)], ret()),
    ]);
    assert_clean(&fixed, "a coalesce result released on both branches");
}

/// Two edges into one block disagreeing about ownership is the shape every seam
/// bug takes; the finding has to name both edges to be actionable.
#[test]
fn join_divergence_names_both_incoming_blocks_and_their_counts() {
    // Both edges write the local, so the disagreement is about how many references
    // they hold rather than about whether it was written at all.
    let body = temp_body(vec![
        block(vec![storage_live(1)], branch(1, 2)),
        block(vec![assign_fresh(1)], goto(3)),
        block(vec![assign_fresh(1), decref(1)], goto(3)),
        block(vec![decref(1), storage_dead(1)], ret()),
    ]);

    let violations = verify_body(&body);
    let divergence = violations
        .iter()
        .find(|violation| violation.message.contains("diverges"))
        .unwrap_or_else(|| {
            panic!(
                "expected a divergence finding, got: {}",
                messages(&violations)
            )
        });
    assert!(
        divergence.message.contains("entering bb3")
            && divergence.message.contains("bb1 owns 1")
            && divergence.message.contains("bb2 owns 0"),
        "the finding must name the merge and both edges, got: {}",
        divergence.message
    );
}

#[test]
fn releasing_more_references_than_are_owned_is_flagged() {
    let body = temp_body(vec![block(
        vec![
            storage_live(1),
            assign_fresh(1),
            decref(1),
            decref(1),
            storage_dead(1),
        ],
        ret(),
    )]);

    let violations = verify_body(&body);
    assert_eq!(violations.len(), 1, "got: {}", messages(&violations));
    assert!(
        violations[0].message.contains("double-release"),
        "got: {}",
        violations[0].message
    );
}

/// A parameter is caller-owned: releasing it in the callee corrupts the caller's
/// count. This check predates the dataflow analysis and must keep firing.
#[test]
fn decref_on_a_parameter_is_flagged() {
    let body = body_of(
        &[void_ty(), string_ty()],
        1,
        vec![block(vec![decref(1)], ret())],
    );

    let violations = verify_body(&body);
    assert_eq!(violations.len(), 1, "got: {}", messages(&violations));
    assert_eq!(violations[0].local, Local(1));
    assert!(
        violations[0].message.contains("caller-owned"),
        "got: {}",
        violations[0].message
    );
}

/// Releasing what a store through a parameter replaced — `self.value = x` in a
/// method — releases a value the object owns, not the caller's own reference.
#[test]
fn decref_through_a_parameter_projection_is_not_flagged() {
    let field = Place {
        local: Local(1),
        projection: vec![miri::mir::PlaceElem::Field(0)],
    };
    let body = body_of(
        &[void_ty(), collection_ty("Tagged", &[])],
        1,
        vec![block(vec![stmt(StatementKind::DecRef(field))], ret())],
    );
    assert_clean(&body, "a DecRef of a field reached through a parameter");
}

/// A back edge must reach a fixpoint rather than looping forever, whether or not
/// the body inside it balances.
#[test]
fn balanced_loop_body_reaches_a_fixpoint_and_reports_nothing() {
    let body = temp_body(vec![
        block(vec![storage_live(1)], goto(1)),
        block(Vec::new(), branch(2, 3)),
        block(vec![assign_fresh(1), decref(1)], goto(1)),
        block(vec![storage_dead(1)], ret()),
    ]);
    assert_clean(&body, "a loop acquiring and releasing each iteration");
}

#[test]
fn loop_acquiring_without_releasing_terminates_and_is_flagged() {
    let body = temp_body(vec![
        block(vec![storage_live(1)], goto(1)),
        block(Vec::new(), branch(2, 3)),
        block(vec![assign_fresh(1)], goto(1)),
        block(vec![storage_dead(1)], ret()),
    ]);

    let violations = verify_body(&body);
    assert!(
        violations
            .iter()
            .all(|violation| violation.local == Local(1)),
        "every finding here is about the same local, got: {}",
        messages(&violations)
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("more than 8")),
        "a count that outgrew the cap must say so rather than name a number it never reached, got: {}",
        messages(&violations)
    );
}

/// Unreachable blocks are dead code, not paths; findings inside them would be
/// noise the reader cannot act on.
#[test]
fn unreachable_blocks_are_not_analysed() {
    let body = temp_body(vec![
        block(
            vec![storage_live(1), assign_fresh(1), decref(1), storage_dead(1)],
            ret(),
        ),
        block(vec![decref(1), decref(1)], ret()),
    ]);
    assert_clean(&body, "a body whose only defect is in an unreachable block");
}

/// Aliasing lowers to `IncRef(source)` immediately followed by
/// `Assign(dest, Copy(source))`, and that increment pays for the destination's
/// reference. Crediting the source instead leaves the destination at zero, so its
/// release reads as a double-release while the source reads as a leak — the defect
/// that made an earlier verifier report every aliasing program in the suite.
#[test]
fn an_incref_funds_the_destination_it_precedes_not_the_local_it_names() {
    let body = body_of(
        &[void_ty(), string_ty(), string_ty()],
        0,
        vec![block(
            vec![
                storage_live(1),
                assign_fresh(1),
                storage_live(2),
                incref(1),
                assign_copy(2, 1),
                decref(2),
                storage_dead(2),
                decref(1),
                storage_dead(1),
            ],
            ret(),
        )],
    );
    assert_clean(&body, "an alias funded by the IncRef that precedes it");
}

/// Without a funding increment the same copy is a borrow, so releasing the alias
/// is releasing a reference nobody acquired.
#[test]
fn a_copy_with_no_funding_incref_is_a_borrow() {
    let body = body_of(
        &[void_ty(), string_ty(), string_ty()],
        0,
        vec![block(
            vec![
                storage_live(1),
                assign_fresh(1),
                storage_live(2),
                assign_copy(2, 1),
                storage_dead(2),
                decref(1),
                storage_dead(1),
            ],
            ret(),
        )],
    );
    assert_clean(&body, "a borrow that is never released");

    // The borrow is released once too often: it is given a reference of its own
    // first, so the state is a local that had one and gave it away — not one that
    // was never written, whose release reads null and does nothing.
    let released = body_of(
        &[void_ty(), string_ty(), string_ty()],
        0,
        vec![block(
            vec![
                storage_live(1),
                assign_fresh(1),
                storage_live(2),
                assign_fresh(2),
                assign_copy(2, 1),
                decref(2),
                decref(2),
                storage_dead(2),
                decref(1),
                storage_dead(1),
            ],
            ret(),
        )],
    );
    let violations = verify_body(&released);
    assert_eq!(violations.len(), 1, "got: {}", messages(&violations));
    assert_eq!(violations[0].local, Local(2));
    assert!(
        violations[0].message.contains("double-release"),
        "got: {}",
        violations[0].message
    );
}

/// A cast re-types a managed value in place rather than copying it, so it carries
/// the source's reference across and the source must not be released as well.
///
/// A local that keeps holding the value past the cast is a second holder, and the
/// retain paying for it is what makes both releases correct.
#[test]
fn a_cast_carries_the_reference_of_the_local_it_reads() {
    let body = body_of(
        &[void_ty(), string_ty(), string_ty()],
        0,
        vec![block(
            vec![
                storage_live(1),
                assign_fresh(1),
                storage_live(2),
                stmt(StatementKind::Assign(
                    place(2),
                    Rvalue::Cast(Box::new(Operand::Copy(place(1))), string_ty()),
                )),
                storage_dead(1),
                decref(2),
                storage_dead(2),
            ],
            ret(),
        )],
    );
    assert_clean(&body, "a cast moving a reference to its destination");

    let second_holder = body_of(
        &[void_ty(), string_ty(), string_ty()],
        0,
        vec![block(
            vec![
                storage_live(1),
                assign_fresh(1),
                storage_live(2),
                incref(1),
                stmt(StatementKind::Assign(
                    place(2),
                    Rvalue::Cast(Box::new(Operand::Copy(place(1))), string_ty()),
                )),
                decref(2),
                storage_dead(2),
                decref(1),
                storage_dead(1),
            ],
            ret(),
        )],
    );
    assert_clean(&second_holder, "a retained cast leaving the source held");
}

/// Field and index reads are borrows by design: moving out of a projection takes
/// the field, not the aggregate holding it.
#[test]
fn moving_out_of_a_projection_does_not_consume_its_base() {
    let string_expr = || {
        miri::ast::expression::Expression::new(
            0,
            miri::ast::expression::ExpressionKind::Type(Box::new(string_ty()), false),
            span(),
        )
    };
    let tuple_ty = Type::new(TypeKind::Tuple(vec![string_expr(), string_expr()]), span());
    let field = Place {
        local: Local(1),
        projection: vec![miri::mir::PlaceElem::Field(0)],
    };
    let body = body_of(
        &[void_ty(), tuple_ty, string_ty()],
        0,
        vec![block(
            vec![
                storage_live(1),
                assign_fresh(1),
                storage_live(2),
                stmt(StatementKind::Assign(
                    place(2),
                    Rvalue::Use(Operand::Move(field)),
                )),
                decref(2),
                storage_dead(2),
                decref(1),
                storage_dead(1),
            ],
            ret(),
        )],
    );
    assert_clean(&body, "a move out of a field leaving its base owned");
}

/// One defect per local: the states after a finding are consequences of it, and
/// reporting each of them buries the cause the reader has to fix.
#[test]
fn a_local_is_reported_once_and_does_not_cascade() {
    let body = temp_body(vec![block(
        vec![
            storage_live(1),
            assign_fresh(1),
            decref(1),
            decref(1),
            decref(1),
            storage_dead(1),
        ],
        ret(),
    )]);

    let violations = verify_body(&body);
    assert_eq!(
        violations.len(),
        1,
        "three releases against one reference is one defect, got: {}",
        messages(&violations)
    );
}

/// Taking a reference out of a local that already gave its away is the same defect
/// as releasing one twice, and has to be reported rather than clamped to zero — a
/// consumed local that silently settles at zero is a release the verifier never
/// mentions.
#[test]
fn consuming_a_local_that_owns_nothing_is_flagged() {
    let body = body_of(
        &[void_ty(), string_ty(), string_ty()],
        0,
        vec![block(
            vec![
                storage_live(1),
                assign_fresh(1),
                decref(1),
                storage_live(2),
                stmt(StatementKind::Assign(
                    place(2),
                    Rvalue::Use(Operand::Move(place(1))),
                )),
                decref(2),
                storage_dead(2),
                storage_dead(1),
            ],
            ret(),
        )],
    );

    let violations = verify_body(&body);
    assert!(
        violations
            .iter()
            .any(|violation| violation.local == Local(1)
                && violation.message.contains("double-release")),
        "moving out of a local that owns nothing must be reported, got: {}",
        messages(&violations)
    );
}

/// A value handed to a container is the container's to release, so the caller
/// holding no reference at scope end is correct rather than a leak.
///
/// This is the shape lowering emits for `list.push(item)`: the item is retained
/// into a temp, the temp is passed by copy, and nothing in the caller releases it.
#[test]
fn a_value_donated_into_a_container_is_not_a_leak() {
    let donating = temp_body(vec![
        block(
            vec![storage_live(1), assign_fresh(1)],
            runtime_call(
                "miri_rt_list_push",
                vec![
                    constant(void_ty(), Literal::Identifier("receiver".to_string())),
                    Operand::Copy(place(1)),
                ],
                2,
                1,
            ),
        ),
        block(vec![storage_dead(1)], ret()),
    ]);
    assert_clean(&donating, "a value donated into a container");
}

/// Donation is per intrinsic and per argument position: the same local passed to
/// the same intrinsic in a position that does not donate is still the caller's.
#[test]
fn only_the_donating_argument_position_transfers_ownership() {
    let wrong_position = temp_body(vec![
        block(
            vec![storage_live(1), assign_fresh(1)],
            runtime_call(
                "miri_rt_list_push",
                vec![
                    Operand::Copy(place(1)),
                    constant(void_ty(), Literal::Identifier("item".to_string())),
                ],
                2,
                1,
            ),
        ),
        block(vec![storage_dead(1)], ret()),
    ]);

    let violations = verify_body(&wrong_position);
    assert!(
        violations
            .iter()
            .any(|violation| violation.local == Local(1) && violation.message.contains("leaks")),
        "the receiver position keeps the caller's reference, got: {}",
        messages(&violations)
    );
}

/// A call to something else entirely donates nothing, so the same fixture minus
/// the intrinsic name still reports the leak — the clean result above comes from
/// the donation table, not from calls being ignored.
#[test]
fn an_ordinary_call_does_not_donate_its_argument() {
    let ordinary = temp_body(vec![
        block(
            vec![storage_live(1), assign_fresh(1)],
            call_returning_void(1, 2, 1),
        ),
        block(vec![storage_dead(1)], ret()),
    ]);

    let violations = verify_body(&ordinary);
    assert!(
        violations
            .iter()
            .any(|violation| violation.local == Local(1) && violation.message.contains("leaks")),
        "a call that is not a donating intrinsic keeps the caller's reference, got: {}",
        messages(&violations)
    );
}

/// A donated argument is spelled `move` when the value came straight from a
/// binding, and `copy` when lowering retained it into a temp first. Both hand the
/// reference over.
#[test]
fn a_donated_argument_transfers_however_it_is_spelled() {
    let moved = temp_body(vec![
        block(
            vec![storage_live(1), assign_fresh(1)],
            runtime_call(
                "miri_rt_map_set",
                vec![
                    constant(void_ty(), Literal::Identifier("receiver".to_string())),
                    constant(void_ty(), Literal::Identifier("key".to_string())),
                    Operand::Move(place(1)),
                ],
                2,
                1,
            ),
        ),
        block(vec![storage_dead(1)], ret()),
    ]);
    assert_clean(&moved, "a donated argument spelled as a move");
}

/// Donating a value the caller does not own is a double-release, not silence:
/// the container will release a reference that was never acquired.
#[test]
fn donating_a_reference_the_caller_does_not_own_is_flagged() {
    let over_donated = temp_body(vec![
        block(
            vec![storage_live(1), assign_fresh(1), decref(1)],
            runtime_call(
                "miri_rt_set_add",
                vec![
                    constant(void_ty(), Literal::Identifier("receiver".to_string())),
                    Operand::Copy(place(1)),
                ],
                2,
                1,
            ),
        ),
        block(vec![storage_dead(1)], ret()),
    ]);

    let violations = verify_body(&over_donated);
    assert!(
        violations
            .iter()
            .any(|violation| violation.local == Local(1)
                && violation.message.contains("double-release")),
        "donating a reference that was already released must be reported, got: {}",
        messages(&violations)
    );
}

/// A call that ends the process takes its path with it: what the caller was still
/// holding is never released, and there is nobody left to release it for.
///
/// This is the shape an assertion lowers to — the failure branch builds the message
/// strings, hands them to the reporting intrinsic, and never comes back.
#[test]
fn a_path_ending_in_a_diverging_call_carries_no_leak() {
    let aborting = temp_body(vec![
        block(Vec::new(), branch(1, 2)),
        block(
            vec![storage_live(1), assign_fresh(1)],
            runtime_call("miri_rt_assert_fail", vec![Operand::Copy(place(1))], 2, 2),
        ),
        block(Vec::new(), ret()),
    ]);
    assert_clean(&aborting, "a path that ends in a diverging call");
}

/// Reading a field out retains it first, and that retain pays for the local the
/// field lands in — not for the value that holds the field.
///
/// Crediting the holder instead makes every field read look like a reference the
/// container acquired and never released, and leaves the reader's own release
/// looking like one release too many.
#[test]
fn a_retained_field_read_credits_the_local_it_lands_in() {
    let string_expr = || {
        miri::ast::expression::Expression::new(
            0,
            miri::ast::expression::ExpressionKind::Type(Box::new(string_ty()), false),
            span(),
        )
    };
    let tuple_ty = Type::new(TypeKind::Tuple(vec![string_expr(), string_expr()]), span());
    let field = Place {
        local: Local(1),
        projection: vec![miri::mir::PlaceElem::Field(0)],
    };
    // The same read spelled as a plain copy and as a cast: lowering emits the cast
    // form when the field's declared type differs from the local receiving it.
    let read_forms = [
        Rvalue::Use(Operand::Copy(field.clone())),
        Rvalue::Cast(Box::new(Operand::Copy(field.clone())), string_ty()),
    ];
    for read in read_forms {
        let body = body_of(
            &[void_ty(), tuple_ty.clone(), string_ty()],
            0,
            vec![block(
                vec![
                    storage_live(1),
                    assign_fresh(1),
                    storage_live(2),
                    stmt(StatementKind::IncRef(field.clone())),
                    stmt(StatementKind::Assign(place(2), read)),
                    decref(2),
                    storage_dead(2),
                    decref(1),
                    storage_dead(1),
                ],
                ret(),
            )],
        );
        assert_clean(&body, "a retained field read");
    }
}

/// A reference retained to be stored inside a value belongs to that value, not to
/// the local it was read from, and not to the value being built.
///
/// Crediting either one turns the storing local's own release into a
/// double-release. The store itself takes nothing over: an aggregate built without
/// a retain is copying values whose references the builder still holds and still
/// releases.
#[test]
fn a_reference_retained_for_a_slot_belongs_to_neither_local() {
    let stored = temp_body(vec![block(
        vec![
            storage_live(1),
            assign_fresh(1),
            incref(1),
            stmt(StatementKind::Assign(
                place(2),
                Rvalue::Aggregate(
                    miri::mir::rvalue::AggregateKind::Tuple,
                    vec![Operand::Copy(place(1))],
                ),
            )),
            decref(1),
            storage_dead(1),
        ],
        ret(),
    )]);
    assert_clean(&stored, "a reference retained for a slot of a value");

    let copied_without_retaining = temp_body(vec![block(
        vec![
            storage_live(1),
            assign_fresh(1),
            stmt(StatementKind::Assign(
                place(2),
                Rvalue::Aggregate(
                    miri::mir::rvalue::AggregateKind::Tuple,
                    vec![Operand::Copy(place(1))],
                ),
            )),
            decref(1),
            storage_dead(1),
        ],
        ret(),
    )]);
    assert_clean(
        &copied_without_retaining,
        "an aggregate built from values the builder still owns",
    );
}

/// Rebinding a variable declared without a value releases an old value that is not
/// there yet, and the release path reads null and does nothing.
///
/// Reporting it would flag every `var x T` followed by an assignment. A local that
/// did hold a reference and released it is a different state, and releasing that
/// one again is still reported.
#[test]
fn releasing_a_local_that_was_never_written_is_not_a_double_release() {
    let declared_then_assigned = temp_body(vec![block(
        vec![
            storage_live(1),
            decref(1),
            assign_fresh(1),
            decref(1),
            storage_dead(1),
        ],
        ret(),
    )]);
    assert_clean(
        &declared_then_assigned,
        "a release of a variable declared without a value",
    );

    let released_twice = temp_body(vec![block(
        vec![
            storage_live(1),
            assign_fresh(1),
            decref(1),
            decref(1),
            storage_dead(1),
        ],
        ret(),
    )]);
    let violations = verify_body(&released_twice);
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("double-release")),
        "releasing a written local twice is still reported, got: {}",
        messages(&violations)
    );
}

/// Rebinding retains the new value before releasing the old one, and the retain
/// pays for the binding rather than for whatever it currently holds.
///
/// Crediting the retain where it is written instead of where the new value lands
/// lets the release in between consume it, leaving the binding owning nothing and
/// its own release reading as one too many.
#[test]
fn a_retain_before_a_rebinding_pays_for_the_new_value() {
    let rebound = temp_body(vec![block(
        vec![
            storage_live(1),
            assign_fresh(1),
            storage_live(2),
            assign_fresh(2),
            incref(1),
            decref(2),
            stmt(StatementKind::Reassign(
                place(2),
                Rvalue::Use(Operand::Copy(place(1))),
            )),
            decref(1),
            storage_dead(1),
            decref(2),
            storage_dead(2),
        ],
        ret(),
    )]);
    assert_clean(&rebound, "a rebinding that releases the old value first");
}

/// An edge that never writes the local does not disagree with one that did.
///
/// The exhaustive switch a match lowers to carries a default edge that falls
/// straight through, leaving the result unwritten. It reads null there, so the
/// release downstream frees what the taken arm produced and does nothing on the
/// default — both paths are correct, and reporting it would flag every match.
#[test]
fn an_edge_that_never_writes_the_local_is_not_a_divergence() {
    let with_a_default_edge = temp_body(vec![
        block(vec![storage_live(1)], branch(1, 2)),
        block(vec![assign_fresh(1)], goto(2)),
        block(vec![decref(1), storage_dead(1)], ret()),
    ]);
    assert_clean(&with_a_default_edge, "a merge with an unwritten edge");
}

/// A call that hands back a reference its container keeps owning gives its
/// destination nothing to release.
///
/// Indexing a map reads through to the entry the map still holds. Counting that as
/// a fresh reference reports every map index as a leak.
#[test]
fn a_borrowed_result_is_not_a_reference_to_release() {
    let indexed = temp_body(vec![
        block(
            Vec::new(),
            runtime_call("miri_rt_map_get_checked", Vec::new(), 1, 1),
        ),
        block(Vec::new(), ret()),
    ]);
    assert_clean(&indexed, "a borrowed result of a map index");
}

/// End to end through the real compiler with findings fatal.
///
/// The program is trivial on purpose: what it proves is that everything reaching
/// the verifier alongside it — the prelude and every stdlib body the program links
/// — is clean too, because strict mode rejects the whole compilation for a finding
/// in any of them.
#[test]
fn a_clean_program_compiles_with_findings_fatal() {
    let result = crate::utils::miri_run_with_env(
        r#"
fn main()
    let a = "hello"
    let b = a
    println(b)
"#,
        "MIRI_VERIFY_MIR",
        "1",
    );
    assert!(
        result.success,
        "aliasing a managed value must pass the verifier: {}",
        result.output()
    );
    assert!(result.stdout.contains("hello"), "got: {}", result.output());
    assert!(
        !result.stderr.contains("MIR invariant violation"),
        "got: {}",
        result.stderr
    );
}

#[test]
fn dealloc_with_exactly_one_ownership_verifies_clean() {
    let clean = temp_body(vec![block(
        vec![
            storage_live(1),
            assign_fresh(1),
            dealloc(1),
            storage_dead(1),
        ],
        ret(),
    )]);
    assert_clean(&clean, "a dealloc with delta = 1");
}

#[test]
fn dealloc_with_delta_two_is_flagged() {
    let over_specialized = temp_body(vec![block(
        vec![
            storage_live(1),
            assign_fresh(1),
            incref(1),
            dealloc(1),
            storage_dead(1),
        ],
        ret(),
    )]);
    let violations = verify_body(&over_specialized);
    assert_eq!(violations.len(), 1, "got: {}", messages(&violations));
    assert_eq!(violations[0].local, Local(1));
    assert!(
        violations[0].message.contains("non-uniquely-owned"),
        "got: {}",
        violations[0].message
    );
}

#[test]
fn dealloc_with_delta_zero_is_flagged() {
    let double_free = temp_body(vec![block(
        vec![
            storage_live(1),
            assign_fresh(1),
            dealloc(1),
            dealloc(1),
            storage_dead(1),
        ],
        ret(),
    )]);
    let violations = verify_body(&double_free);
    assert_eq!(violations.len(), 1, "got: {}", messages(&violations));
    assert_eq!(violations[0].local, Local(1));
    // The message matters, not just the count: releasing an unowned place is a
    // double release under `DecRef`, but freeing one unconditionally is a
    // uniqueness failure. Asserting the wording keeps this test discriminating
    // if the two rules are ever collapsed back together.
    assert!(
        violations[0].message.contains("non-uniquely-owned"),
        "got: {}",
        violations[0].message
    );
}

#[test]
fn dealloc_in_one_arm_and_decref_in_the_other_verifies_clean() {
    // A pass that proves uniqueness on only one path may release through
    // `Dealloc` there and leave `DecRef` on the other. Both arms release the
    // single reference exactly once, so the join carries no ownership and the
    // mixed shape is legal.
    let mixed = body_of(
        &[void_ty(), string_ty()],
        0,
        vec![
            block(vec![storage_live(1), assign_fresh(1)], branch(1, 2)),
            block(vec![dealloc(1)], goto(3)),
            block(vec![decref(1)], goto(3)),
            block(vec![storage_dead(1)], ret()),
        ],
    );
    assert_clean(&mixed, "a dealloc on one arm and a decref on the other");
}

/// A body whose local 1 is a `gpu`-resident parameter carrying `handle`, local 2
/// a host binding of the same type, and local 3 an unmanaged call destination.
///
/// The parameter borrows its caller's device buffer, which the caller may have
/// launched on, so it starts unfenced.
fn cross_residency_body(handle: u64, blocks: Vec<BasicBlockData>) -> Body {
    let mut body = body_of(&[void_ty(), string_ty(), string_ty(), void_ty()], 1, blocks);
    body.local_decls[1].residency = BindingResidency::Gpu;
    body.local_decls[1].device_handle = Some(DeviceHandleId(handle));
    body
}

/// [`cross_residency_body`] with local 1 a binding the body declares rather than
/// a parameter: it has no device buffer until a launch touches it.
fn declared_binding_body(handle: u64, blocks: Vec<BasicBlockData>) -> Body {
    let mut body = cross_residency_body(handle, blocks);
    body.arg_count = 0;
    body
}

/// The handle argument a readback call carries, spelled as lowering spells it.
fn handle_argument(handle: u64) -> Operand {
    constant(
        Type::new(TypeKind::Int, span()),
        Literal::Integer(IntegerLiteral::I64(handle as i64)),
    )
}

#[test]
fn copying_a_gpu_binding_to_the_host_without_a_readback_is_reported() {
    // The defect 15.48 recorded: the copy runs, the host binding keeps its own
    // initial contents, and the program exits 0 having transferred nothing.
    let unfenced = cross_residency_body(7, vec![block(vec![assign_copy(2, 1)], ret())]);

    let violations = verify_cross_residency_readback(&unfenced);
    assert_eq!(
        violations.len(),
        1,
        "expected one finding, got: {}",
        messages(&violations)
    );
    assert_eq!(violations[0].local, Local(1));
    assert!(
        violations[0].message.contains("no readback"),
        "got: {}",
        violations[0].message
    );
}

#[test]
fn a_readback_before_the_copy_verifies_clean() {
    let fenced = cross_residency_body(
        7,
        vec![
            block(
                Vec::new(),
                runtime_call(
                    "miri_gpu_readback",
                    vec![handle_argument(7), Operand::Copy(place(1))],
                    3,
                    1,
                ),
            ),
            block(vec![assign_copy(2, 1)], ret()),
        ],
    );

    let violations = verify_cross_residency_readback(&fenced);
    assert!(
        violations.is_empty(),
        "a fenced copy must verify clean, got: {}",
        messages(&violations)
    );
}

/// The readback has to name the handle being copied: fencing a different
/// binding's buffer leaves this one's transfer as silent as no fence at all.
#[test]
fn a_readback_of_another_handle_does_not_fence_this_copy() {
    let wrong_handle = cross_residency_body(
        7,
        vec![
            block(
                Vec::new(),
                runtime_call(
                    "miri_gpu_readback",
                    vec![handle_argument(9), Operand::Copy(place(1))],
                    3,
                    1,
                ),
            ),
            block(vec![assign_copy(2, 1)], ret()),
        ],
    );

    let violations = verify_cross_residency_readback(&wrong_handle);
    assert_eq!(
        violations.len(),
        1,
        "expected one finding, got: {}",
        messages(&violations)
    );
}

/// A body copying local 1 into local 2, a gpu binding carrying `target_handle`.
fn gpu_to_gpu_copy(target_handle: u64) -> Body {
    let mut body = cross_residency_body(7, vec![block(vec![assign_copy(2, 1)], ret())]);
    body.local_decls[2].residency = BindingResidency::Gpu;
    body.local_decls[2].device_handle = Some(DeviceHandleId(target_handle));
    body
}

/// A binding that takes over the source's device buffer (`gpu var b = a`)
/// keeps the value on the device, so nothing has to be fenced for it.
#[test]
fn a_move_into_a_binding_sharing_the_device_buffer_needs_no_readback() {
    assert_fenced(&gpu_to_gpu_copy(7), "a move that keeps the device buffer");
}

/// A copy into a gpu binding with a buffer of its own (`b = a`) copies the
/// source's host array, which lags whatever the device did to it.
#[test]
fn a_copy_into_a_gpu_binding_with_its_own_buffer_needs_a_readback() {
    assert_one_unfenced_read(&gpu_to_gpu_copy(8));
}

/// Until a launch touches it a declared binding has no device buffer, so its
/// host value is its only copy and reading it needs no readback.
#[test]
fn a_declared_binding_no_launch_touched_needs_no_readback() {
    let untouched = declared_binding_body(7, vec![block(vec![assign_copy(2, 1)], ret())]);
    assert_fenced(&untouched, "a binding no launch touched");
}

#[test]
fn a_launch_leaves_a_declared_binding_unfenced() {
    let launched = declared_binding_body(
        7,
        vec![
            block(Vec::new(), launch_over_binding(7, 1)),
            block(vec![assign_copy(2, 1)], ret()),
        ],
    );
    assert_one_unfenced_read(&launched);
}

/// A fresh activation opens the handle with no device buffer, so it fences the
/// handle the way a readback does — a binding redeclared on each turn of a loop
/// starts current however the previous turn left it.
#[test]
fn an_activation_fences_its_handle() {
    let reactivated = cross_residency_body(
        7,
        vec![
            block(
                Vec::new(),
                runtime_call("miri_gpu_acquire", vec![handle_argument(7)], 3, 1),
            ),
            block(vec![assign_copy(2, 1)], ret()),
        ],
    );
    assert_fenced(&reactivated, "a copy after a fresh activation");
}

/// An upload into another binding's buffer copies the source's host array, so
/// the source has to be fenced as for any other host read.
#[test]
fn uploading_one_gpu_binding_into_another_needs_a_readback_of_the_source() {
    let upload = cross_residency_body(
        7,
        vec![
            block(
                Vec::new(),
                runtime_call(
                    "miri_gpu_upload",
                    vec![handle_argument(8), Operand::Copy(place(1))],
                    3,
                    1,
                ),
            ),
            block(Vec::new(), ret()),
        ],
    );
    assert_one_unfenced_read(&upload);
}

/// Capturing a gpu binding into a closure copies its host array into the
/// closure's environment, a host boundary the same as `let h = g`: without a
/// readback the closure holds the host array's initial values.
fn closure_capture(dest: usize, captured: usize) -> Statement {
    let closure_ty = Type::new(TypeKind::RawPtr, span());
    stmt(StatementKind::Assign(
        place(dest),
        Rvalue::Aggregate(
            AggregateKind::Closure("main_lambda_0".into(), closure_ty),
            vec![Operand::Copy(place(captured))],
        ),
    ))
}

#[test]
fn capturing_a_gpu_binding_without_a_readback_is_reported() {
    let unfenced = cross_residency_body(7, vec![block(vec![closure_capture(2, 1)], ret())]);

    let violations = verify_cross_residency_readback(&unfenced);
    assert_eq!(
        violations.len(),
        1,
        "expected one finding, got: {}",
        messages(&violations)
    );
    assert_eq!(violations[0].local, Local(1));
    assert!(
        violations[0].message.contains("captured"),
        "got: {}",
        violations[0].message
    );
}

#[test]
fn a_readback_before_the_capture_verifies_clean() {
    let fenced = cross_residency_body(
        7,
        vec![
            block(
                Vec::new(),
                runtime_call(
                    "miri_gpu_readback",
                    vec![handle_argument(7), Operand::Copy(place(1))],
                    3,
                    1,
                ),
            ),
            block(vec![closure_capture(2, 1)], ret()),
        ],
    );

    let violations = verify_cross_residency_readback(&fenced);
    assert!(
        violations.is_empty(),
        "a fenced capture must verify clean, got: {}",
        messages(&violations)
    );
}

/// A readback of local 1's buffer under `handle`, continuing at `target`.
fn readback_of_binding(handle: u64, target: usize) -> Terminator {
    runtime_call(
        "miri_gpu_readback",
        vec![handle_argument(handle), Operand::Copy(place(1))],
        3,
        target,
    )
}

/// `dest = (source, 0)`: a tuple built around a whole local.
fn tuple_around(dest: usize, source: usize) -> Statement {
    stmt(StatementKind::Assign(
        place(dest),
        Rvalue::Aggregate(
            AggregateKind::Tuple,
            vec![
                Operand::Copy(place(source)),
                constant(
                    Type::new(TypeKind::Int, span()),
                    Literal::Integer(IntegerLiteral::I64(0)),
                ),
            ],
        ),
    ))
}

/// A kernel launch over local 1's buffer under `handle`, continuing at `target`.
fn launch_over_binding(handle: u64, target: usize) -> Terminator {
    let launch_args = GpuLaunchArgs::new(
        vec![Operand::Copy(place(1))],
        vec![Some(DeviceHandleId(handle))],
        vec![false],
        vec![false],
    )
    .expect("one capture, one entry per metadata vector");
    let int_operand = || {
        constant(
            Type::new(TypeKind::Int, span()),
            Literal::Integer(IntegerLiteral::I64(1)),
        )
    };
    terminator(TerminatorKind::GpuLaunch {
        kernel: constant(void_ty(), Literal::Identifier("kernel_0".to_string())),
        grid: int_operand(),
        block: int_operand(),
        launch_args,
        scalar_args: Vec::new(),
        uniform_bound_x: None,
        uniform_bound_y: None,
        uniform_bound_z: None,
        uniform_start_x: None,
        uniform_start_y: None,
        uniform_start_z: None,
        destination: place(3),
        target: Some(BasicBlock(target)),
    })
}

/// A call passing local 1 to a body specialized to launch on its buffer.
fn specialized_call_on_binding(handle: u64, target: usize) -> Terminator {
    terminator(TerminatorKind::Call {
        func: callee(void_ty()),
        args: vec![Operand::Copy(place(1))],
        out_args: Vec::new(),
        arg_handles: vec![Some(DeviceHandleId(handle))],
        destination: place(3),
        target: Some(BasicBlock(target)),
    })
}

fn assert_one_unfenced_read(body: &Body) {
    let violations = verify_cross_residency_readback(body);
    assert_eq!(
        violations.len(),
        1,
        "expected one finding, got: {}",
        messages(&violations)
    );
    assert_eq!(violations[0].local, Local(1));
    assert!(
        violations[0].message.contains("no readback"),
        "got: {}",
        violations[0].message
    );
}

fn assert_fenced(body: &Body, what: &str) {
    let violations = verify_cross_residency_readback(body);
    assert!(
        violations.is_empty(),
        "{} must verify clean, got: {}",
        what,
        messages(&violations)
    );
}

/// `let t = (g, 1)` puts the host array into the tuple as surely as `let h = g`
/// copies it: without a readback the tuple holds the initial values.
#[test]
fn a_gpu_binding_built_into_a_tuple_without_a_readback_is_reported() {
    let unfenced = cross_residency_body(7, vec![block(vec![tuple_around(2, 1)], ret())]);
    assert_one_unfenced_read(&unfenced);
}

#[test]
fn a_readback_before_the_tuple_verifies_clean() {
    let fenced = cross_residency_body(
        7,
        vec![
            block(Vec::new(), readback_of_binding(7, 1)),
            block(vec![tuple_around(2, 1)], ret()),
        ],
    );
    assert_fenced(&fenced, "a fenced tuple");
}

/// Comparing a gpu binding with a host value reads the host array.
#[test]
fn a_gpu_binding_read_by_an_operator_without_a_readback_is_reported() {
    let comparison = stmt(StatementKind::Assign(
        place(3),
        Rvalue::BinaryOp(
            miri::mir::BinOp::Eq,
            Box::new(Operand::Copy(place(1))),
            Box::new(Operand::Copy(place(2))),
        ),
    ));
    let unfenced = cross_residency_body(7, vec![block(vec![comparison], ret())]);
    assert_one_unfenced_read(&unfenced);
}

/// A call argument is the type checker's residency gate to judge: what reaches
/// a call is either a device buffer or a binding whose callee reads only its
/// length, and MIR cannot tell the second from a data read.
#[test]
fn a_gpu_binding_passed_to_a_call_is_left_to_the_residency_gate() {
    let length_only = cross_residency_body(
        7,
        vec![
            block(Vec::new(), launch_over_binding(7, 1)),
            block(Vec::new(), call_returning_void(1, 3, 2)),
            block(Vec::new(), ret()),
        ],
    );
    assert_fenced(&length_only, "a call argument");
}

/// An upload leaves the host array and the device buffer holding the same
/// values, so a copy after it reads what the device holds.
#[test]
fn an_upload_after_the_launch_fences_the_copy() {
    let uploaded = cross_residency_body(
        7,
        vec![
            block(Vec::new(), launch_over_binding(7, 1)),
            block(
                Vec::new(),
                runtime_call(
                    "miri_gpu_upload",
                    vec![handle_argument(7), Operand::Copy(place(2))],
                    3,
                    2,
                ),
            ),
            block(vec![assign_copy(2, 1)], ret()),
        ],
    );
    assert_fenced(&uploaded, "a copy after an upload");
}

/// A residency-specialized callee launches on the caller's device buffer and
/// never reads the host array, so the argument needs no fence.
#[test]
fn a_gpu_binding_passed_to_a_residency_specialized_call_needs_no_readback() {
    let on_device = cross_residency_body(
        7,
        vec![
            block(Vec::new(), specialized_call_on_binding(7, 1)),
            block(Vec::new(), ret()),
        ],
    );
    assert_fenced(&on_device, "a residency-specialized argument");
}

/// A launch reads its capture on the device, not the host array.
#[test]
fn a_kernel_launch_over_a_gpu_binding_needs_no_readback() {
    let on_device = cross_residency_body(
        7,
        vec![
            block(Vec::new(), launch_over_binding(7, 1)),
            block(Vec::new(), ret()),
        ],
    );
    assert_fenced(&on_device, "a launch capture");
}

/// A readback anywhere in the body used to fence every copy in it. One that
/// runs after the copy has written nothing the copy could see.
#[test]
fn a_readback_after_the_copy_does_not_fence_it() {
    let late = cross_residency_body(
        7,
        vec![
            block(vec![assign_copy(2, 1)], readback_of_binding(7, 1)),
            block(Vec::new(), ret()),
        ],
    );
    assert_one_unfenced_read(&late);
}

/// A launch after the readback leaves results on the device that the host
/// array does not hold, so the copy after the launch is unfenced again.
#[test]
fn a_launch_between_the_readback_and_the_copy_reopens_the_fence() {
    let relaunched = cross_residency_body(
        7,
        vec![
            block(Vec::new(), readback_of_binding(7, 1)),
            block(Vec::new(), launch_over_binding(7, 2)),
            block(vec![assign_copy(2, 1)], ret()),
        ],
    );
    assert_one_unfenced_read(&relaunched);
}

/// A call specialized to launch on the buffer reopens the fence as a launch
/// does.
#[test]
fn a_specialized_call_between_the_readback_and_the_copy_reopens_the_fence() {
    let relaunched = cross_residency_body(
        7,
        vec![
            block(Vec::new(), readback_of_binding(7, 1)),
            block(Vec::new(), specialized_call_on_binding(7, 2)),
            block(vec![assign_copy(2, 1)], ret()),
        ],
    );
    assert_one_unfenced_read(&relaunched);
}

/// A readback on one arm fences nothing on the path through the other arm.
#[test]
fn a_readback_on_one_branch_does_not_fence_a_copy_after_the_join() {
    let one_arm = cross_residency_body(
        7,
        vec![
            block(Vec::new(), branch(1, 3)),
            block(Vec::new(), readback_of_binding(7, 2)),
            block(Vec::new(), goto(3)),
            block(vec![assign_copy(2, 1)], ret()),
        ],
    );
    assert_one_unfenced_read(&one_arm);
}

#[test]
fn a_readback_on_every_branch_fences_a_copy_after_the_join() {
    let both_arms = cross_residency_body(
        7,
        vec![
            block(Vec::new(), branch(1, 2)),
            block(Vec::new(), readback_of_binding(7, 3)),
            block(Vec::new(), readback_of_binding(7, 3)),
            block(vec![assign_copy(2, 1)], ret()),
        ],
    );
    assert_fenced(&both_arms, "a copy fenced on every incoming path");
}

/// Launch, read back, copy, repeat: every turn of the loop fences its own copy.
#[test]
fn a_loop_that_relaunches_and_reads_back_each_turn_verifies_clean() {
    let looping = cross_residency_body(
        7,
        vec![
            block(Vec::new(), goto(1)),
            block(Vec::new(), launch_over_binding(7, 2)),
            block(Vec::new(), readback_of_binding(7, 3)),
            block(vec![assign_copy(2, 1)], branch(1, 4)),
            block(Vec::new(), ret()),
        ],
    );
    assert_fenced(&looping, "a copy fenced on every turn of a loop");
}

/// A gpu scalar reads back through a one-element array seeded from the
/// scalar's host value. The seed is the readback's own destination, written
/// before the readback fills it, not a host copy of the result.
#[test]
fn seeding_a_scalar_readback_wrapper_needs_no_readback() {
    let mut scalar = cross_residency_body(
        7,
        vec![
            block(
                vec![stmt(StatementKind::Assign(
                    place(4),
                    Rvalue::Aggregate(AggregateKind::Array, vec![Operand::Copy(place(1))]),
                ))],
                runtime_call(
                    "miri_gpu_readback",
                    vec![handle_argument(7), Operand::Copy(place(4))],
                    3,
                    1,
                ),
            ),
            block(vec![assign_copy(2, 1)], ret()),
        ],
    );
    scalar.new_local(LocalDecl::new(string_ty(), span()));
    assert_fenced(&scalar, "a scalar readback wrapper");
}

/// A built-in collection class reference, e.g. `Set<String>`, spelled the way
/// the type checker records an instantiated receiver.
fn collection_ty(class_name: &str, args: &[TypeKind]) -> Type {
    let args = args
        .iter()
        .map(|kind| miri::ast::factory::type_expr_non_null(Type::new(kind.clone(), span())))
        .collect();
    Type::new(TypeKind::Custom(class_name.to_string(), Some(args)), span())
}

/// Locals: 0 the return slot, 1 a receiver typed `receiver`, 2 the call's result.
/// The body's one call hands the receiver to `symbol`.
fn collection_call_body(receiver: Type, symbol: &str) -> Body {
    body_of(
        &[void_ty(), receiver.clone(), receiver],
        0,
        vec![
            block(
                Vec::new(),
                runtime_call(symbol, vec![Operand::Copy(place(1))], 2, 1),
            ),
            block(Vec::new(), ret()),
        ],
    )
}

/// `Set.map` is declared on `Set` and takes a function value, so its shared body
/// never releases what that function returns: a `Set<String>` receiver has to
/// reach the per-instantiation symbol instead.
#[test]
fn a_shared_set_transform_over_reference_counted_elements_is_reported() {
    let body = collection_call_body(collection_ty("Set", &[TypeKind::String]), "Set_map");

    let violations = verify_collection_element_ownership(&body, &HashSet::new());
    assert_eq!(
        violations.len(),
        1,
        "expected one finding, got: {}",
        messages(&violations)
    );
    assert!(
        violations[0].message.contains("Set_map"),
        "the finding must name the shared symbol, got: {}",
        messages(&violations)
    );
}

#[test]
fn a_per_instantiation_set_transform_verifies_clean() {
    let body = collection_call_body(collection_ty("Set", &[TypeKind::String]), "Set_map__String");

    let violations = verify_collection_element_ownership(&body, &HashSet::new());
    assert!(
        violations.is_empty(),
        "a per-instantiation body must verify clean, got: {}",
        messages(&violations)
    );
}

/// A map's values are released one by one as surely as its keys, so a
/// reference-counted value alone puts the receiver under the rule.
#[test]
fn a_shared_map_transform_over_reference_counted_values_is_reported() {
    let body = collection_call_body(
        collection_ty("Map", &[TypeKind::Int, TypeKind::String]),
        "Map_filter",
    );

    let violations = verify_collection_element_ownership(&body, &HashSet::new());
    assert_eq!(
        violations.len(),
        1,
        "expected one finding, got: {}",
        messages(&violations)
    );
}

#[test]
fn a_shared_set_transform_over_plain_integers_verifies_clean() {
    let body = collection_call_body(collection_ty("Set", &[TypeKind::Int]), "Set_map");

    let violations = verify_collection_element_ownership(&body, &HashSet::new());
    assert!(
        violations.is_empty(),
        "nothing reference-counted is handed over, got: {}",
        messages(&violations)
    );
}

/// A method that settles element ownership through the runtime is correct in
/// its shared body, and the caller's exemption says so.
#[test]
fn a_runtime_backed_map_method_verifies_clean() {
    let body = collection_call_body(
        collection_ty("Map", &[TypeKind::String, TypeKind::Int]),
        "Map_get",
    );
    let exempt = HashSet::from(["Map_get".to_string()]);

    let violations = verify_collection_element_ownership(&body, &exempt);
    assert!(
        violations.is_empty(),
        "an exempt symbol must verify clean, got: {}",
        messages(&violations)
    );
}

/// Locals: 0 the return slot, 1 a receiver typed `receiver`, 2 an element typed
/// `element`, 3 the call's result. The body's one call hands the element to
/// `symbol`.
fn element_call_body(receiver: Type, element: Type, symbol: &str) -> Body {
    body_of(
        &[void_ty(), receiver, element, void_ty()],
        0,
        vec![
            block(
                Vec::new(),
                runtime_call(
                    symbol,
                    vec![Operand::Copy(place(1)), Operand::Copy(place(2))],
                    3,
                    1,
                ),
            ),
            block(Vec::new(), ret()),
        ],
    )
}

/// A set copies its whole slot out of the buffer the caller spills the element
/// into, and the caller sizes that buffer from the element's own type: eight
/// bytes handed to a sixteen-byte slot leave half of it filled by whatever lay
/// beside it, which no lookup can match.
#[test]
fn an_element_narrower_than_the_slot_it_is_stored_in_is_reported() {
    let body = element_call_body(
        collection_ty("Set", &[TypeKind::I128]),
        Type::new(TypeKind::Int, span()),
        "miri_rt_set_add",
    );

    let violations = verify_body(&body);
    assert_eq!(
        violations.len(),
        1,
        "expected one finding, got: {}",
        messages(&violations)
    );
    assert!(
        violations[0].message.contains("miri_rt_set_add")
            && violations[0].message.contains("8 bytes")
            && violations[0].message.contains("16 bytes"),
        "the finding must name the symbol and both widths, got: {}",
        messages(&violations)
    );
}

#[test]
fn an_element_spelled_at_the_slot_type_verifies_clean() {
    let body = element_call_body(
        collection_ty("Set", &[TypeKind::I128]),
        Type::new(TypeKind::I128, span()),
        "miri_rt_set_add",
    );
    assert_clean(&body, "an element as wide as the slot it is stored in");
}

/// A map value goes to the second element position, so a width check reading
/// only the key would pass a body storing a truncated value.
#[test]
fn a_map_value_narrower_than_its_slot_is_reported() {
    let body = body_of(
        &[
            void_ty(),
            collection_ty("Map", &[TypeKind::Int, TypeKind::I128]),
            Type::new(TypeKind::Int, span()),
            Type::new(TypeKind::Int, span()),
            void_ty(),
        ],
        0,
        vec![
            block(
                Vec::new(),
                runtime_call(
                    "miri_rt_map_set",
                    vec![
                        Operand::Copy(place(1)),
                        Operand::Copy(place(2)),
                        Operand::Copy(place(3)),
                    ],
                    4,
                    1,
                ),
            ),
            block(Vec::new(), ret()),
        ],
    );

    let violations = verify_body(&body);
    assert_eq!(
        violations.len(),
        1,
        "the key fills its slot and only the value does not, got: {}",
        messages(&violations)
    );
    assert!(
        violations[0].message.contains("argument 2"),
        "the finding must name the argument that misses its slot, or a check that \
         read the key against the value's slot would read the same, got: {}",
        messages(&violations)
    );
}

/// A key filling its slot while the value does not is the other way round, and
/// has to be told apart from it by the finding alone.
#[test]
fn a_map_key_narrower_than_its_slot_names_the_key_argument() {
    let body = body_of(
        &[
            void_ty(),
            collection_ty("Map", &[TypeKind::I128, TypeKind::Int]),
            Type::new(TypeKind::Int, span()),
            Type::new(TypeKind::Int, span()),
            void_ty(),
        ],
        0,
        vec![
            block(
                Vec::new(),
                runtime_call(
                    "miri_rt_map_set",
                    vec![
                        Operand::Copy(place(1)),
                        Operand::Copy(place(2)),
                        Operand::Copy(place(3)),
                    ],
                    4,
                    1,
                ),
            ),
            block(Vec::new(), ret()),
        ],
    );

    let violations = verify_body(&body);
    assert_eq!(
        violations.len(),
        1,
        "the value fills its slot and only the key does not, got: {}",
        messages(&violations)
    );
    assert!(
        violations[0].message.contains("argument 1"),
        "the finding must name the key argument, got: {}",
        messages(&violations)
    );
}

/// A canonical collection spelling reaches the width table as a shape it has no
/// entry for. The pass reports what it can read and stays silent about the rest;
/// it never fails on a body it was asked to check.
#[test]
fn a_slot_spelled_as_a_canonical_collection_verifies_clean() {
    let inner = TypeKind::Set(Box::new(miri::ast::factory::type_expr_non_null(Type::new(
        TypeKind::Int,
        span(),
    ))));
    let body = element_call_body(
        collection_ty("Set", &[inner]),
        Type::new(TypeKind::Int, span()),
        "miri_rt_set_add",
    );
    assert_clean(&body, "a slot spelled as a canonical collection");
}

/// A body lowered once for every instantiation spells its element as its own
/// type parameter, which has no width until the instantiation gives it one.
#[test]
fn an_element_typed_by_an_unpinned_type_parameter_verifies_clean() {
    let mut body = element_call_body(
        collection_ty("Set", &[TypeKind::I128]),
        Type::new(TypeKind::Custom("T".to_string(), None), span()),
        "miri_rt_set_add",
    );
    body.type_params.insert("T".to_string());
    assert_clean(&body, "an element typed by an unpinned type parameter");
}

fn bool_ty() -> Type {
    Type::new(TypeKind::Boolean, span())
}

/// `flag = value`.
fn assign_flag(flag: usize, value: bool) -> Statement {
    stmt(StatementKind::Assign(
        place(flag),
        Rvalue::Use(constant(bool_ty(), Literal::Boolean(value))),
    ))
}

/// `switchInt(flag)`: to `when_clear` on false, else to `when_set`.
fn switch_on_flag(flag: usize, when_clear: usize, when_set: usize) -> Terminator {
    terminator(TerminatorKind::SwitchInt {
        discr: Operand::Copy(place(flag)),
        targets: vec![(Discriminant::bool_false(), BasicBlock(when_clear))],
        otherwise: BasicBlock(when_set),
    })
}

/// A declared binding launched on, then copied behind a test of flag local 4:
/// read back when the flag is set, copied directly when it is clear. The flag
/// is set after the launch when `set_after_launch`, and is known to the body as
/// the handle's stale flag when `registered`.
fn flag_guarded_copy(set_after_launch: bool, registered: bool) -> Body {
    let after_launch = if set_after_launch {
        vec![assign_flag(4, true)]
    } else {
        Vec::new()
    };
    let mut body = declared_binding_body(
        7,
        vec![
            block(vec![assign_flag(4, false)], launch_over_binding(7, 1)),
            block(after_launch, switch_on_flag(4, 3, 2)),
            block(Vec::new(), readback_of_binding(7, 4)),
            block(vec![assign_copy(2, 1)], ret()),
            block(vec![assign_flag(4, false)], goto(3)),
        ],
    );
    body.new_local(LocalDecl::new(bool_ty(), span()));
    if registered {
        body.device_stale_flags.insert(Local(4), DeviceHandleId(7));
    }
    body
}

/// The clear branch of a flag set by every launch fences the read: the device
/// has not run since the flag was last cleared by a readback.
#[test]
fn the_clear_branch_of_a_stale_flag_fences_the_read() {
    assert_fenced(&flag_guarded_copy(true, true), "a copy on a clear flag");
}

/// A flag the launch did not set proves nothing when found clear.
#[test]
fn a_flag_left_clear_by_a_launch_does_not_fence_the_read() {
    assert_one_unfenced_read(&flag_guarded_copy(false, true));
}

/// Only the flags the readback pass keeps count: branching on any other
/// boolean fences nothing.
#[test]
fn a_boolean_the_body_does_not_keep_as_a_stale_flag_fences_nothing() {
    assert_one_unfenced_read(&flag_guarded_copy(true, false));
}

/// Readback calls in `body`.
fn readback_calls(body: &Body) -> usize {
    body.basic_blocks
        .iter()
        .filter(|block| {
            matches!(
                block.terminator.as_ref().map(|t| &t.kind),
                Some(TerminatorKind::Call { func, .. })
                    if func.called_symbol() == Some("miri_gpu_readback")
            )
        })
        .count()
}

/// A loop whose first block reads a declared binding and whose back edge
/// launches on it: the read is behind on some entries and current on the first,
/// so the pass guards it with a flag, which it has to set before the loop
/// without running that assignment again on every turn.
#[test]
fn the_readback_pass_keeps_a_flag_for_a_read_at_the_head_of_a_loop() {
    let mut looping = declared_binding_body(
        7,
        vec![
            block(vec![assign_copy(2, 1)], branch(1, 2)),
            block(Vec::new(), launch_over_binding(7, 0)),
            block(Vec::new(), ret()),
        ],
    );
    assert_one_unfenced_read(&looping);

    miri::mir::residency::insert_readbacks(&mut looping);

    assert_fenced(&looping, "the pass's output");
    assert_eq!(readback_calls(&looping), 1, "one guarded readback");
    assert_eq!(looping.device_stale_flags.len(), 1, "one flag for handle 7");
    let enters_the_entry_block = looping
        .basic_blocks
        .iter()
        .filter_map(|block| block.terminator.as_ref())
        .any(|t| t.successors().contains(&BasicBlock(0)));
    assert!(
        !enters_the_entry_block,
        "the flag's initialization must run once, before the loop:\n{looping}"
    );
}

/// A read every path reaches behind reads back unconditionally, and a second
/// read with no launch between finds the host array current.
#[test]
fn the_readback_pass_reads_back_once_for_two_reads_after_a_launch() {
    let mut twice = declared_binding_body(
        7,
        vec![
            block(Vec::new(), launch_over_binding(7, 1)),
            block(vec![assign_copy(2, 1), tuple_around(2, 1)], ret()),
        ],
    );

    miri::mir::residency::insert_readbacks(&mut twice);

    assert_fenced(&twice, "the pass's output");
    assert_eq!(readback_calls(&twice), 1);
    assert!(twice.device_stale_flags.is_empty(), "no read needs a flag");
}
