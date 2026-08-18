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
    let body = temp_body(vec![
        block(vec![storage_live(1)], branch(1, 2)),
        block(vec![assign_fresh(1)], goto(3)),
        block(Vec::new(), goto(3)),
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

    let released = body_of(
        &[void_ty(), string_ty(), string_ty()],
        0,
        vec![block(
            vec![
                storage_live(1),
                assign_fresh(1),
                storage_live(2),
                assign_copy(2, 1),
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
                    Rvalue::Cast(Box::new(Operand::Move(field)), string_ty()),
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

/// Taking a reference out of a local that owns none is the same defect as
/// releasing one twice, and has to be reported rather than clamped to zero — a
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

/// End to end through the real compiler with findings fatal.
///
/// Ignored, and it is the criterion for un-ignoring: no program can pass strict
/// mode today, however clean its own code, because every program links the prelude
/// and `List.enumerate`/`List.zip` build their result by donating each element into
/// a collection — which the verifier still reads as a reference acquired per
/// iteration and never released. That is what keeps the harness warn-only. Once the
/// table of which argument each collection intrinsic takes ownership of lands, this
/// test passing is what says the pass is ready to be a gate.
#[ignore]
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
        !result.stderr.contains("RC invariant violations"),
        "got: {}",
        result.stderr
    );
}
