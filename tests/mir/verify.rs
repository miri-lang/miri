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

use miri::ast::literal::Literal;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::verify::{verify_body, VerificationViolation};
use miri::mir::{
    Body, Constant, Discriminant, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
};

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
        !result.stderr.contains("RC invariant violation"),
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
