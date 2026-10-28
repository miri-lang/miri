// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the pass that gives a loop compared with a bound other than a
//! length a second, checked-free copy, entered only when the bound is known
//! not to pass the length of every list the loop indexes.
//!
//! Each hand-built body is the stable-collections test loop with its guard
//! `i < n` or `i <= n` against a bound `n` the body does not compute.

use crate::mir::optimization::in_bounds::{
    compare, reading_at_an_offset, stepped, with_two_pointers, BOUND, GUARD, OFFSET_INDEX,
};
use crate::mir::optimization::stable_collections::{
    assign, call, indexed, int, loop_over_a_list, terminator, ty, BELOW, ELEMENT, INDEX, LIST,
    LOOP_BODY, LOOP_HEADER, SPARE, SPARE_LIST,
};
use miri::ast::types::TypeKind;
use miri::error::syntax::Span;
use miri::mir::optimization::in_bounds::record_in_bounds;
use miri::mir::optimization::loop_versioning::version_bounded_loops;
use miri::mir::optimization::stable_collections::record_stable_collections;
use miri::mir::{
    BasicBlock, BasicBlockData, BinOp, Body, Discriminant, ExecutionModel, InBounds, LocalDecl,
    Operand, Place, Rvalue, StatementKind, TerminatorKind,
};

const EXIT: usize = 3;
/// The blocks the pass appends after the four of the test loop: the copy of
/// its header, the copy of its step, then the guard.
const COPY_HEADER: usize = 4;
const COPY_BODY: usize = 5;
const FIRST_GUARD: usize = 6;

/// `i = 0; while i <op> n: x = list[i]; i += 1`, for a bound `n` the body
/// does not compute.
fn loop_bounded_by(op: BinOp) -> Body {
    let mut body = loop_over_a_list();
    body.local_decls.extend([
        LocalDecl::new(ty(TypeKind::Int), Span::default()),
        LocalDecl::new(ty(TypeKind::Boolean), Span::default()),
    ]);
    body.basic_blocks[LOOP_HEADER].statements =
        vec![assign(Place::new(BELOW), compare(op, INDEX, BOUND))];
    body
}

/// `body` versioned, or a failure naming what the pass did not do.
fn versioned(mut body: Body) -> Body {
    assert!(version_bounded_loops(&mut body), "not versioned:\n{body}");
    body
}

/// Whether the pass leaves `body` exactly as it was.
fn is_left_alone(body: Body) -> bool {
    let before = body.to_string();
    let mut after = body;
    !version_bounded_loops(&mut after) && after.to_string() == before
}

/// The switch ending `block` of `body`: `(condition, true target, otherwise)`.
fn switch_of(body: &Body, block: usize) -> (Operand, usize, usize) {
    let Some(TerminatorKind::SwitchInt {
        discr,
        targets,
        otherwise,
    }) = body.basic_blocks[block]
        .terminator
        .as_ref()
        .map(|t| &t.kind)
    else {
        panic!("bb{block} does not switch:\n{body}");
    };
    let [(value, target)] = targets.as_slice() else {
        panic!("bb{block} switches more than two ways:\n{body}");
    };
    assert_eq!(*value, Discriminant::bool_true());
    (discr.clone(), target.0, otherwise.0)
}

/// The comparison the guard ending `block` computes: `(op, lhs, rhs)` of its
/// last statement, after checking the one before measures `LIST`.
fn guard_comparison(body: &Body, block: usize) -> (BinOp, String, String) {
    let statements = &body.basic_blocks[block].statements;
    let [measure, compared] = statements.as_slice() else {
        panic!("bb{block} is no guard:\n{body}");
    };
    let StatementKind::Assign(length, Rvalue::Len(measured)) = &measure.kind else {
        panic!("bb{block} measures nothing:\n{body}");
    };
    assert_eq!(measured.local, LIST);
    let StatementKind::Assign(_, Rvalue::BinaryOp(op, lhs, rhs)) = &compared.kind else {
        panic!("bb{block} compares nothing:\n{body}");
    };
    assert_eq!(rhs.to_string(), Operand::Copy(length.clone()).to_string());
    (*op, lhs.to_string(), rhs.to_string())
}

fn proofs_of(mut body: Body) -> Vec<InBounds> {
    record_stable_collections(&mut body);
    record_in_bounds(&mut body);
    body.in_bounds
}

#[test]
fn test_a_loop_below_a_bound_is_entered_through_a_guard_against_the_length() {
    let body = versioned(loop_bounded_by(BinOp::Lt));
    assert_eq!(body.basic_blocks.len(), FIRST_GUARD + 1, "{body}");
    let entry = body.basic_blocks[0].terminator.as_ref().map(|t| &t.kind);
    assert!(
        matches!(entry, Some(TerminatorKind::Goto { target }) if target.0 == FIRST_GUARD),
        "{body}"
    );
    let (op, lhs, _) = guard_comparison(&body, FIRST_GUARD);
    assert_eq!(op, BinOp::Le);
    assert_eq!(lhs, Operand::Copy(Place::new(BOUND)).to_string());
    let (_, passes, fails) = switch_of(&body, FIRST_GUARD);
    assert_eq!((passes, fails), (COPY_HEADER, LOOP_HEADER), "{body}");
    assert_eq!(switch_of(&body, COPY_HEADER).1, COPY_BODY, "{body}");
    assert_eq!(switch_of(&body, COPY_HEADER).2, EXIT, "{body}");
    assert_eq!(
        body.basic_blocks[COPY_BODY].statements,
        body.basic_blocks[LOOP_BODY].statements
    );
}

#[test]
fn test_only_the_copy_entered_through_the_guard_is_proven_in_bounds() {
    let proofs = proofs_of(versioned(loop_bounded_by(BinOp::Lt)));
    assert_eq!(
        proofs,
        vec![InBounds {
            block: BasicBlock(COPY_BODY),
            statement_index: 0,
            collection: LIST,
            index: INDEX,
        }]
    );
}

#[test]
fn test_a_loop_up_to_a_bound_is_guarded_by_a_strict_comparison() {
    let body = versioned(loop_bounded_by(BinOp::Le));
    assert_eq!(guard_comparison(&body, FIRST_GUARD).0, BinOp::Lt);
    assert_eq!(proofs_of(body).len(), 1);
}

#[test]
fn test_a_loop_compared_with_the_length_itself_is_left_alone() {
    assert!(is_left_alone(loop_over_a_list()));
}

#[test]
fn test_a_loop_writing_its_bound_is_left_alone() {
    let mut body = loop_bounded_by(BinOp::Lt);
    body.basic_blocks[LOOP_BODY]
        .statements
        .push(assign(Place::new(BOUND), Rvalue::Use(int(3))));
    assert!(is_left_alone(body));
}

#[test]
fn test_a_loop_rebinding_its_list_is_left_alone() {
    let mut body = loop_bounded_by(BinOp::Lt);
    body.basic_blocks[LOOP_BODY].statements.push(assign(
        Place::new(LIST),
        Rvalue::Use(Operand::Move(Place::new(SPARE_LIST))),
    ));
    assert!(is_left_alone(body));
}

#[test]
fn test_a_loop_pushing_onto_its_list_is_left_alone() {
    let mut body = loop_bounded_by(BinOp::Lt);
    let push = call(
        Operand::runtime(miri::runtime_fns::rt::LIST_PUSH, Span::default()),
        vec![Operand::Copy(Place::new(LIST)), int(1)],
        SPARE,
        LOOP_HEADER,
    );
    body.basic_blocks[LOOP_BODY].terminator = Some(push);
    assert!(is_left_alone(body));
}

#[test]
fn test_a_loop_holding_a_phi_is_left_alone() {
    let mut body = loop_bounded_by(BinOp::Lt);
    let phi = Rvalue::Phi(vec![
        (int(0), BasicBlock(0)),
        (int(1), BasicBlock(LOOP_BODY)),
    ]);
    body.basic_blocks[LOOP_HEADER]
        .statements
        .insert(0, assign(Place::new(SPARE), phi));
    assert!(is_left_alone(body));
}

#[test]
fn test_a_gpu_body_is_left_alone() {
    let mut body = loop_bounded_by(BinOp::Lt);
    body.execution_model = ExecutionModel::GpuKernel;
    assert!(is_left_alone(body));
}

/// The test loop inside an outer loop over the same list and bound,
/// `while i < n: x = list[i]` followed by the inner loop, whose exit jumps
/// back to the outer header.
fn nested_loops() -> Body {
    let mut body = loop_bounded_by(BinOp::Lt);
    let outer_header = body.basic_blocks.len();
    let outer_body = outer_header + 1;
    let done = outer_header + 2;
    body.basic_blocks[0].terminator = Some(terminator(TerminatorKind::Goto {
        target: BasicBlock(outer_header),
    }));
    body.basic_blocks[EXIT].terminator = Some(terminator(TerminatorKind::Goto {
        target: BasicBlock(outer_header),
    }));
    let mut header = BasicBlockData::new(Some(terminator(TerminatorKind::SwitchInt {
        discr: Operand::Copy(Place::new(GUARD)),
        targets: vec![(Discriminant::bool_true(), BasicBlock(outer_body))],
        otherwise: BasicBlock(done),
    })));
    header
        .statements
        .push(assign(Place::new(GUARD), compare(BinOp::Lt, INDEX, BOUND)));
    let mut step = BasicBlockData::new(Some(terminator(TerminatorKind::Goto {
        target: BasicBlock(LOOP_HEADER),
    })));
    step.statements.push(assign(
        Place::new(ELEMENT),
        Rvalue::Use(Operand::Copy(indexed(LIST))),
    ));
    let finish = BasicBlockData::new(Some(terminator(TerminatorKind::Return)));
    body.basic_blocks.extend([header, step, finish]);
    body
}

#[test]
fn test_only_the_innermost_of_nested_loops_is_versioned() {
    let before = nested_loops().basic_blocks.len();
    let body = versioned(nested_loops());
    // Two copied blocks of the inner loop and one guard; the outer loop of
    // three blocks is not copied.
    assert_eq!(body.basic_blocks.len(), before + 3, "{body}");
    let guard = body.basic_blocks.len() - 1;
    assert_eq!(switch_of(&body, guard).2, LOOP_HEADER, "{body}");
}

#[test]
fn test_versioning_numbers_blocks_and_locals_the_same_on_every_run() {
    let first = versioned(nested_loops()).to_string();
    let second = versioned(nested_loops()).to_string();
    assert_eq!(first, second);
}

#[test]
fn test_a_versioned_body_keeps_every_statement_of_the_checked_loop() {
    let original = loop_bounded_by(BinOp::Lt);
    let body = versioned(original.clone());
    for block in [LOOP_HEADER, LOOP_BODY, EXIT] {
        assert_eq!(
            body.basic_blocks[block].statements,
            original.basic_blocks[block].statements
        );
    }
}

/// The body of the function named `.{name}` in the benchmark `workload`,
/// through every MIR pass.
fn workload_function(workload: &str, name: &str) -> Body {
    let path = format!(
        "{}/bench/workloads/{workload}/{workload}.mi",
        env!("CARGO_MANIFEST_DIR")
    );
    let source = std::fs::read_to_string(&path).expect("the workload should be readable");
    miri::pipeline::Pipeline::new()
        .get_mir_bodies_with_rc(&source)
        .expect("pipeline should lower the program")
        .into_iter()
        .find(|(symbol, _)| symbol.ends_with(&format!(".{name}")))
        .map(|(_, body)| body)
        .expect("the workload defines the function")
}

/// The accesses `body` records in bounds, as the text of the statement each
/// sits in, after checking the verifier finds every one proven.
fn proven_statements(body: &Body) -> Vec<String> {
    let violations: Vec<String> = miri::mir::verify::verify_in_bounds(body)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(violations.is_empty(), "{violations:?}\n{body}");
    (body.in_bounds.iter())
        .filter_map(|proof| {
            body.basic_blocks[proof.block.0]
                .statements
                .get(proof.statement_index)
                .map(ToString::to_string)
        })
        .collect()
}

#[test]
fn test_the_sieve_marks_multiples_without_a_bounds_check() {
    let body = workload_function("sieve", "kernel");
    let proven = proven_statements(&body);
    assert!(
        proven
            .iter()
            .any(|text| text.contains("] = const Boolean(true)")),
        "{proven:?}\n{body}"
    );
}

#[test]
fn test_matmul_reads_a_row_and_a_row_of_the_other_matrix_without_a_bounds_check() {
    let body = workload_function("matmul", "kernel");
    let proven = proven_statements(&body);
    assert!(
        proven.iter().any(|text| text.contains("Mul(")),
        "{proven:?}\n{body}"
    );
    assert!(proven.len() >= 2, "{proven:?}\n{body}");
}

#[test]
fn test_fannkuch_copies_its_permutation_without_a_bounds_check() {
    let body = workload_function("fannkuch-redux", "kernel");
    let proven = proven_statements(&body);
    assert!(
        proven.iter().any(|text| text.contains("] = _3[")),
        "{proven:?}\n{body}"
    );
}

/// The proofs of `body` once versioned, after checking the verifier finds
/// each proven.
fn checked_proofs_of(body: Body) -> Vec<InBounds> {
    let mut body = versioned(body);
    record_stable_collections(&mut body);
    record_in_bounds(&mut body);
    let violations: Vec<String> = miri::mir::verify::verify_in_bounds(&body)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(violations.is_empty(), "{violations:?}\n{body}");
    body.in_bounds
}

#[test]
fn test_a_loop_reading_one_past_its_index_is_guarded_by_a_bound_below_the_length() {
    // `i < n` and `n < L` make `i + 1 < L`.
    let loop_ = reading_at_an_offset(loop_bounded_by(BinOp::Lt), stepped(BinOp::Add, INDEX, 1));
    let body = versioned(loop_.clone());
    let (op, lhs, _) = guard_comparison(&body, FIRST_GUARD);
    assert_eq!(op, BinOp::Lt);
    assert_eq!(lhs, Operand::Copy(Place::new(BOUND)).to_string());
    assert_eq!(
        checked_proofs_of(loop_),
        vec![InBounds {
            block: BasicBlock(COPY_BODY),
            statement_index: 1,
            collection: LIST,
            index: OFFSET_INDEX,
        }]
    );
}

#[test]
fn test_a_loop_up_to_its_bound_reading_one_past_it_is_guarded_two_below_the_length() {
    // `i <= n` and `n <= L - 2` make `i + 1 < L`; the guard measures
    // `m = L - 2`, which cannot overflow for a length of at least 0.
    let loop_ = reading_at_an_offset(loop_bounded_by(BinOp::Le), stepped(BinOp::Add, INDEX, 1));
    let body = versioned(loop_.clone());
    let statements = &body.basic_blocks[FIRST_GUARD].statements;
    let texts: Vec<String> = statements.iter().map(ToString::to_string).collect();
    assert_eq!(texts.len(), 3, "{body}");
    assert!(
        texts[1].contains("Sub(") && texts[1].contains("const Integer(I64(2))"),
        "{texts:?}"
    );
    assert!(texts[2].contains("Le(_8,"), "{texts:?}");
    assert_eq!(checked_proofs_of(loop_).len(), 1);
}

/// The test loop reversing `list` between `i` and `hi`, its high pointer
/// stepped by `step`.
fn reversal(step: BinOp) -> Body {
    with_two_pointers(loop_bounded_by(BinOp::Lt), step)
}

#[test]
fn test_a_reversal_is_guarded_by_its_high_pointer_below_the_length() {
    let body = versioned(reversal(BinOp::Sub));
    let (op, lhs, _) = guard_comparison(&body, FIRST_GUARD);
    assert_eq!(op, BinOp::Lt);
    assert_eq!(lhs, Operand::Copy(Place::new(BOUND)).to_string());
    let indexes: Vec<_> = checked_proofs_of(reversal(BinOp::Sub))
        .iter()
        .map(|proof| (proof.block.0, proof.index))
        .collect();
    assert_eq!(indexes, vec![(COPY_BODY, INDEX), (COPY_BODY, BOUND)]);
}

#[test]
fn test_a_reversal_whose_high_pointer_counts_up_is_left_alone() {
    assert!(is_left_alone(reversal(BinOp::Add)));
}

#[test]
fn test_a_reversal_whose_high_pointer_is_also_set_is_left_alone() {
    let mut body = reversal(BinOp::Sub);
    body.basic_blocks[LOOP_BODY]
        .statements
        .push(assign(Place::new(BOUND), Rvalue::Use(int(3))));
    assert!(is_left_alone(body));
}

#[test]
fn test_a_reversal_whose_low_pointer_counts_down_is_left_alone() {
    let mut body = reversal(BinOp::Sub);
    let step = body.basic_blocks[LOOP_BODY]
        .statements
        .iter_mut()
        .find(|s| s.to_string().starts_with("_2 = Add("))
        .expect("the step moves the low pointer");
    *step = assign(Place::new(INDEX), stepped(BinOp::Sub, INDEX, 1));
    assert!(is_left_alone(body));
}

/// The test loop followed by a second one over the same list, the first
/// leaving straight into the second's header.
fn loops_in_sequence() -> Body {
    let mut body = loop_bounded_by(BinOp::Lt);
    let second_body = body.basic_blocks.len();
    let done = second_body + 1;
    let mut header = BasicBlockData::new(Some(terminator(TerminatorKind::SwitchInt {
        discr: Operand::Copy(Place::new(BELOW)),
        targets: vec![(Discriminant::bool_true(), BasicBlock(second_body))],
        otherwise: BasicBlock(done),
    })));
    header.statements = body.basic_blocks[LOOP_HEADER].statements.clone();
    let mut step = body.basic_blocks[LOOP_BODY].clone();
    step.terminator = Some(terminator(TerminatorKind::Goto {
        target: BasicBlock(EXIT),
    }));
    let finish = BasicBlockData::new(Some(terminator(TerminatorKind::Return)));
    body.basic_blocks[EXIT] = header;
    body.basic_blocks.extend([step, finish]);
    body
}

#[test]
fn test_the_copy_of_a_loop_leaving_into_another_enters_its_guard() {
    let body = versioned(loops_in_sequence());
    // The first loop's copy (two blocks) and guard come first, then the
    // second's copy and guard; the first copy leaves into the second guard.
    let first_copy_header = 6;
    let second_guard = body.basic_blocks.len() - 1;
    assert_eq!(
        switch_of(&body, first_copy_header).2,
        second_guard,
        "{body}"
    );
    assert_eq!(switch_of(&body, LOOP_HEADER).2, second_guard, "{body}");
}

#[test]
fn test_a_reversal_copying_its_high_pointer_from_a_step_made_after_the_copy_is_left_alone() {
    // `hi = t; t = hi - 1`: `hi` takes the step of the iteration before, and
    // on the first one whatever `t` held before the loop.
    let mut body = reversal(BinOp::Sub);
    let statements = &mut body.basic_blocks[LOOP_BODY].statements;
    let last = statements.len() - 1;
    statements.swap(last - 1, last);
    assert!(is_left_alone(body));
}
