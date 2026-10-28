// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the pass that records the element accesses a loop guard proves
//! in bounds. Each hand-built body is the stable-collections test loop
//! `while i < list.length(): x = list[i]; i += 1`, changed in one way.

use crate::mir::optimization::stable_collections::{
    assign, call, int, list_of, loop_over_a_list, statement, terminator, ty, BELOW, ELEMENT, INDEX,
    LENGTH, LIST, LOOP_BODY, LOOP_HEADER, SPARE, SPARE_LIST,
};
use miri::ast::types::TypeKind;
use miri::error::syntax::Span;
use miri::mir::optimization::in_bounds::{record_in_bounds, INTEGER_ARITHMETIC_TRAPS_ON_OVERFLOW};
use miri::mir::optimization::stable_collections::record_stable_collections;
use miri::mir::symbol::Symbol;
use miri::mir::{
    BasicBlock, BinOp, Body, Discriminant, ExecutionModel, InBounds, Local, LocalDecl, Operand,
    Place, Rvalue, StatementKind, TerminatorKind,
};
use miri::pipeline::Pipeline;

/// The in-bounds entries `body` gets once its stable collections are
/// recorded.
pub(crate) fn proofs_of(mut body: Body) -> Vec<InBounds> {
    record_stable_collections(&mut body);
    record_in_bounds(&mut body);
    body.in_bounds
}

/// The bound the loop of [`loop_under_a_guard`] compares its index with.
pub(crate) const BOUND: Local = Local(8);
/// The condition the guard in front of that loop switches on.
pub(crate) const GUARD: Local = Local(9);

/// `lhs op rhs` over two whole locals.
pub(crate) fn compare(op: BinOp, lhs: Local, rhs: Local) -> Rvalue {
    Rvalue::BinaryOp(
        op,
        Box::new(Operand::Copy(Place::new(lhs))),
        Box::new(Operand::Copy(Place::new(rhs))),
    )
}

/// The test loop compared with `n` rather than the length:
/// `i = 0; L = list.length(); g = guard; if g: while i <op> n: x = list[i]; i += 1`,
/// where `guard` is computed into `g` in front of the loop.
pub(crate) fn loop_under_a_guard(guard: Rvalue, op: BinOp) -> Body {
    let mut body = loop_over_a_list();
    body.local_decls.extend([
        LocalDecl::new(ty(TypeKind::Int), Span::default()),
        LocalDecl::new(ty(TypeKind::Boolean), Span::default()),
    ]);
    body.basic_blocks[0].statements.extend([
        assign(Place::new(LENGTH), Rvalue::Len(Place::new(LIST))),
        assign(Place::new(GUARD), guard),
    ]);
    body.basic_blocks[0].terminator = Some(terminator(TerminatorKind::SwitchInt {
        discr: Operand::Copy(Place::new(GUARD)),
        targets: vec![(Discriminant::bool_true(), BasicBlock(LOOP_HEADER))],
        otherwise: BasicBlock(3),
    }));
    body.basic_blocks[LOOP_HEADER].statements =
        vec![assign(Place::new(BELOW), compare(op, INDEX, BOUND))];
    body
}

fn step_by(op: BinOp) -> Rvalue {
    Rvalue::BinaryOp(
        op,
        Box::new(Operand::Copy(Place::new(INDEX))),
        Box::new(int(1)),
    )
}

#[test]
fn test_a_read_below_the_guarded_length_is_proven_in_bounds() {
    let proofs = proofs_of(loop_over_a_list());
    assert_eq!(
        proofs,
        vec![InBounds {
            block: BasicBlock(LOOP_BODY),
            statement_index: 0,
            collection: LIST,
            index: INDEX,
        }]
    );
}

#[test]
fn test_a_less_or_equal_guard_proves_nothing() {
    let mut body = loop_over_a_list();
    let StatementKind::Assign(_, Rvalue::BinaryOp(op, ..)) =
        &mut body.basic_blocks[LOOP_HEADER].statements[1].kind
    else {
        panic!("the header compares the index to the length");
    };
    *op = BinOp::Le;
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_an_index_changed_between_the_guard_and_the_access_is_not_proven() {
    let mut body = loop_over_a_list();
    body.basic_blocks[LOOP_BODY]
        .statements
        .insert(0, assign(Place::new(INDEX), step_by(BinOp::Add)));
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_an_index_that_starts_negative_is_not_proven() {
    let mut body = loop_over_a_list();
    body.basic_blocks[0].statements[0] = assign(Place::new(INDEX), Rvalue::Use(int(-1)));
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_an_index_that_counts_down_is_not_proven() {
    let mut body = loop_over_a_list();
    body.basic_blocks[LOOP_BODY].statements[1] = assign(Place::new(INDEX), step_by(BinOp::Sub));
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_an_index_held_by_a_parameter_is_not_proven() {
    let mut body = loop_over_a_list();
    body.arg_count = INDEX.0;
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_a_length_replaced_between_the_comparison_and_the_switch_proves_nothing() {
    let mut body = loop_over_a_list();
    body.basic_blocks[LOOP_HEADER]
        .statements
        .push(assign(Place::new(LENGTH), Rvalue::Use(int(9))));
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_a_length_whose_storage_ended_before_the_comparison_proves_nothing() {
    let mut body = loop_over_a_list();
    body.basic_blocks[LOOP_HEADER]
        .statements
        .insert(1, statement(StatementKind::StorageDead(Place::new(LENGTH))));
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_a_length_that_is_not_the_lists_proves_nothing() {
    let mut body = loop_over_a_list();
    body.basic_blocks[LOOP_HEADER].statements[0] = assign(Place::new(LENGTH), Rvalue::Use(int(3)));
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_a_push_between_the_length_and_the_access_proves_nothing() {
    let mut body = loop_over_a_list();
    let header = body.basic_blocks[LOOP_HEADER].terminator.take();
    let push = call(
        Operand::runtime(miri::runtime_fns::rt::LIST_PUSH, Span::default()),
        vec![Operand::Copy(Place::new(LIST)), int(1)],
        SPARE,
        body.basic_blocks.len(),
    );
    body.basic_blocks[LOOP_HEADER].terminator = Some(push);
    let mut after = miri::mir::BasicBlockData::new(header);
    after.statements.clear();
    body.basic_blocks.push(after);
    assert!(proofs_of(body).is_empty());
}

/// The proofs of the test loop once its step starts with a copy-on-write of
/// a second list with elements of `element`, between the guard and the read.
/// The list is recorded stable over the loop before the copy is added, so
/// only the in-bounds dataflow decides what the copy loses.
fn proofs_after_copying_a_list_of(element: TypeKind) -> Vec<InBounds> {
    let mut body = loop_over_a_list();
    body.local_decls[SPARE_LIST.0] = LocalDecl::new(list_of(element), Span::default());
    record_stable_collections(&mut body);
    let read = body.basic_blocks.len();
    let copy = call(
        Operand::runtime(miri::runtime_fns::rt::LIST_COW, Span::default()),
        vec![Operand::Move(Place::new(SPARE_LIST))],
        SPARE_LIST,
        read,
    );
    let step = std::mem::replace(
        &mut body.basic_blocks[LOOP_BODY],
        miri::mir::BasicBlockData::new(Some(copy)),
    );
    body.basic_blocks.push(step);
    for record in &mut body.stable_collections {
        record.blocks.push(BasicBlock(read));
    }
    record_in_bounds(&mut body);
    body.in_bounds
}

#[test]
fn test_a_copy_on_write_of_another_list_of_ints_keeps_the_proof() {
    assert_eq!(proofs_after_copying_a_list_of(TypeKind::Int).len(), 1);
}

#[test]
fn test_a_copy_of_a_list_whose_elements_may_run_a_clone_method_proves_nothing() {
    let instances = TypeKind::Custom("Plain".to_string(), None);
    assert!(proofs_after_copying_a_list_of(instances).is_empty());
}

#[test]
fn test_an_index_below_a_bound_no_greater_than_the_length_is_proven() {
    let body = loop_under_a_guard(compare(BinOp::Le, BOUND, LENGTH), BinOp::Lt);
    assert_eq!(
        proofs_of(body),
        vec![InBounds {
            block: BasicBlock(LOOP_BODY),
            statement_index: 0,
            collection: LIST,
            index: INDEX,
        }]
    );
}

#[test]
fn test_an_index_up_to_a_bound_no_greater_than_the_length_is_not_proven() {
    let body = loop_under_a_guard(compare(BinOp::Le, BOUND, LENGTH), BinOp::Le);
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_an_index_up_to_a_bound_below_the_length_is_proven() {
    let body = loop_under_a_guard(compare(BinOp::Lt, BOUND, LENGTH), BinOp::Le);
    assert_eq!(proofs_of(body).len(), 1);
}

#[test]
fn test_a_guard_comparing_the_length_with_the_bound_the_wrong_way_proves_nothing() {
    let body = loop_under_a_guard(compare(BinOp::Le, LENGTH, BOUND), BinOp::Lt);
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_a_bound_changed_in_the_loop_proves_nothing() {
    let mut body = loop_under_a_guard(compare(BinOp::Le, BOUND, LENGTH), BinOp::Lt);
    let grow = Rvalue::BinaryOp(
        BinOp::Add,
        Box::new(Operand::Copy(Place::new(BOUND))),
        Box::new(int(1)),
    );
    body.basic_blocks[LOOP_BODY]
        .statements
        .push(assign(Place::new(BOUND), grow));
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_a_call_of_the_program_in_the_loop_proves_nothing() {
    let user_call = call(
        Operand::function(Symbol::entry(), Span::default()),
        Vec::new(),
        SPARE,
        LOOP_HEADER,
    );
    let mut body = loop_over_a_list();
    body.basic_blocks[LOOP_BODY].terminator = Some(user_call);
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_a_gpu_body_records_no_proof() {
    let mut body = loop_over_a_list();
    body.execution_model = ExecutionModel::GpuKernel;
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_a_sum_of_non_negative_operands_is_non_negative_only_because_it_traps_on_overflow() {
    // Were integer `+` to wrap, `i + 1` could turn negative and the loop
    // above would not be proven; the rule is tied to this constant.
    assert!(INTEGER_ARITHMETIC_TRAPS_ON_OVERFLOW);
    assert_eq!(proofs_of(loop_over_a_list()).len(), 1);
}

#[test]
fn test_every_access_of_the_nbody_step_is_proven_in_bounds() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/bench/workloads/nbody/nbody.mi"
    );
    let source = std::fs::read_to_string(path).expect("nbody source should be readable");
    let body = Pipeline::new()
        .get_mir_bodies_with_rc(&source)
        .expect("pipeline should lower the program")
        .into_iter()
        .find(|(name, _)| name.ends_with(".advance"))
        .map(|(_, body)| body)
        .expect("nbody has an advance step");
    let mut accesses = 0;
    for (index, data) in body.basic_blocks.iter().enumerate() {
        for (position, statement) in data.statements.iter().enumerate() {
            let text = statement.to_string();
            if text.contains("_4[") {
                accesses += 1;
                assert!(
                    body.in_bounds
                        .iter()
                        .any(|p| p.block.0 == index && p.statement_index == position),
                    "bb{index}[{position}] `{text}` is not proven:\n{body}"
                );
            }
        }
    }
    assert!(accesses > 20, "{body}");
    let violations: Vec<String> = miri::mir::verify::verify_in_bounds(&body)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(violations.is_empty(), "{violations:?}");
}

/// The local the step of [`reading_at_an_offset`] reads the list at.
pub(crate) const OFFSET_INDEX: Local = Local(10);

/// `local + value`, or `local - value`, by `op`.
pub(crate) fn stepped(op: BinOp, local: Local, value: i64) -> Rvalue {
    Rvalue::BinaryOp(
        op,
        Box::new(Operand::Copy(Place::new(local))),
        Box::new(int(value)),
    )
}

/// `list[index]` for any index local.
pub(crate) fn element_at(index: Local) -> Rvalue {
    Rvalue::Use(Operand::Copy(Place {
        local: LIST,
        projection: vec![miri::mir::PlaceElem::Index(index)],
    }))
}

/// `body`, whose loop step reads `list[i]` first, reading `list[x]` instead
/// after computing `x = offset` in front of it.
pub(crate) fn reading_at_an_offset(mut body: Body, offset: Rvalue) -> Body {
    body.local_decls
        .push(LocalDecl::new(ty(TypeKind::Int), Span::default()));
    let step = &mut body.basic_blocks[LOOP_BODY].statements;
    step[0] = assign(Place::new(ELEMENT), element_at(OFFSET_INDEX));
    step.insert(0, assign(Place::new(OFFSET_INDEX), offset));
    body
}

/// The guarded test loop `while i <op> n` entered when `n <= L - gap`,
/// computed as `m = L - gap; g = Le(n, m)` in front of the loop.
pub(crate) fn loop_under_a_gap(gap: i64, op: BinOp) -> Body {
    let mut body = loop_under_a_guard(compare(BinOp::Le, BOUND, SPARE), op);
    body.basic_blocks[0].statements.insert(
        2,
        assign(Place::new(SPARE), stepped(BinOp::Sub, LENGTH, gap)),
    );
    body
}

/// The read of `list[x]` at the start of the step, as the pass records it.
fn offset_read() -> InBounds {
    InBounds {
        block: BasicBlock(LOOP_BODY),
        statement_index: 1,
        collection: LIST,
        index: OFFSET_INDEX,
    }
}

#[test]
fn test_an_index_one_past_one_below_a_bound_below_the_length_is_proven() {
    let body = loop_under_a_guard(compare(BinOp::Lt, BOUND, LENGTH), BinOp::Lt);
    let body = reading_at_an_offset(body, stepped(BinOp::Add, INDEX, 1));
    assert_eq!(proofs_of(body), vec![offset_read()]);
}

#[test]
fn test_an_index_one_past_one_below_a_bound_at_most_the_length_is_not_proven() {
    // `i < n <= L` leaves `i + 1 == L` possible.
    let body = loop_under_a_guard(compare(BinOp::Le, BOUND, LENGTH), BinOp::Lt);
    let body = reading_at_an_offset(body, stepped(BinOp::Add, INDEX, 1));
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_an_index_one_past_one_up_to_a_bound_below_the_length_is_not_proven() {
    // `i <= n < L` leaves `i + 1 == L` possible.
    let body = loop_under_a_guard(compare(BinOp::Lt, BOUND, LENGTH), BinOp::Le);
    let body = reading_at_an_offset(body, stepped(BinOp::Add, INDEX, 1));
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_an_offset_within_the_gap_below_the_length_is_proven() {
    // `i < n <= L - 2` makes `i + 2 < L`.
    let body = reading_at_an_offset(
        loop_under_a_gap(2, BinOp::Lt),
        stepped(BinOp::Add, INDEX, 2),
    );
    assert_eq!(proofs_of(body), vec![offset_read()]);
}

#[test]
fn test_an_offset_past_the_gap_below_the_length_is_not_proven() {
    // `i < n <= L - 2` leaves `i + 3 == L` possible.
    let body = reading_at_an_offset(
        loop_under_a_gap(2, BinOp::Lt),
        stepped(BinOp::Add, INDEX, 3),
    );
    assert!(proofs_of(body).is_empty());
}

#[test]
fn test_a_negative_offset_is_not_proven() {
    // `i - 1` and `i + -1` are -1 on the first iteration.
    for offset in [
        stepped(BinOp::Add, INDEX, -1),
        stepped(BinOp::Sub, INDEX, 1),
    ] {
        let body = loop_under_a_guard(compare(BinOp::Lt, BOUND, LENGTH), BinOp::Lt);
        assert!(proofs_of(reading_at_an_offset(body, offset)).is_empty());
    }
}

/// `while i < hi: x = list[i]; x = list[hi]; i += 1; t = hi <step> 1; hi = t`,
/// entered when `hi <guard> L`: the two-pointer loop of a reversal.
pub(crate) fn two_pointer_loop(guard: BinOp, step: BinOp) -> Body {
    let body = loop_under_a_guard(compare(guard, BOUND, LENGTH), BinOp::Lt);
    with_two_pointers(body, step)
}

/// `body`'s loop over `i < hi` also reading `list[hi]` and stepping `hi` by
/// `step` through a temporary.
pub(crate) fn with_two_pointers(mut body: Body, step: BinOp) -> Body {
    let statements = &mut body.basic_blocks[LOOP_BODY].statements;
    statements.insert(1, assign(Place::new(ELEMENT), element_at(BOUND)));
    statements.extend([
        assign(Place::new(SPARE), stepped(step, BOUND, 1)),
        assign(
            Place::new(BOUND),
            Rvalue::Use(Operand::Copy(Place::new(SPARE))),
        ),
    ]);
    body
}

/// The indexes `proofs` records reads of `list` at.
fn proven_indexes(proofs: &[InBounds]) -> Vec<Local> {
    proofs.iter().map(|proof| proof.index).collect()
}

#[test]
fn test_both_pointers_of_a_reversal_below_the_length_are_proven() {
    let proofs = proofs_of(two_pointer_loop(BinOp::Lt, BinOp::Sub));
    assert_eq!(proven_indexes(&proofs), vec![INDEX, BOUND]);
}

#[test]
fn test_a_high_pointer_starting_at_the_length_is_not_proven() {
    // `hi <= L` admits `hi == L` on the first iteration; `i < hi <= L` still
    // keeps the low pointer inside.
    let proofs = proofs_of(two_pointer_loop(BinOp::Le, BinOp::Sub));
    assert_eq!(proven_indexes(&proofs), vec![INDEX]);
}

#[test]
fn test_a_high_pointer_that_counts_up_is_not_proven() {
    assert!(proofs_of(two_pointer_loop(BinOp::Lt, BinOp::Add)).is_empty());
}

#[test]
fn test_a_high_pointer_above_a_low_pointer_that_may_be_negative_is_not_proven() {
    // Nothing bounds `hi` from below once `i` may start at -1.
    let mut body = two_pointer_loop(BinOp::Lt, BinOp::Sub);
    body.basic_blocks[0].statements[0] = assign(Place::new(INDEX), Rvalue::Use(int(-1)));
    assert!(proofs_of(body).is_empty());
}

/// The condition the header of [`switching_on_a_copy`] compares into.
pub(crate) const COMPARED: Local = Local(8);

/// The test loop whose header compares into `c'` and switches on a copy
/// `c = c'`, as `and` and `or` are lowered, with `between` run after the
/// comparison and before the copy.
pub(crate) fn switching_on_a_copy(between: Vec<miri::mir::Statement>) -> Body {
    let mut body = loop_over_a_list();
    body.local_decls
        .push(LocalDecl::new(ty(TypeKind::Boolean), Span::default()));
    let header = &mut body.basic_blocks[LOOP_HEADER].statements;
    header[1] = assign(Place::new(COMPARED), compare(BinOp::Lt, INDEX, LENGTH));
    header.extend(between);
    header.push(assign(
        Place::new(BELOW),
        Rvalue::Use(Operand::Copy(Place::new(COMPARED))),
    ));
    body
}

#[test]
fn test_a_switch_on_a_copy_of_the_comparison_proves_the_read() {
    assert_eq!(proofs_of(switching_on_a_copy(Vec::new())).len(), 1);
}

#[test]
fn test_a_switch_on_a_copy_after_the_index_changed_proves_nothing() {
    let between = vec![assign(Place::new(INDEX), Rvalue::Use(int(9)))];
    assert!(proofs_of(switching_on_a_copy(between)).is_empty());
}

#[test]
fn test_a_switch_on_a_copy_of_a_copy_of_the_comparison_proves_nothing() {
    // One whole-local copy is followed, not a chain of them:
    // `c'' = Lt(i, L); c' = c''; c = c'`.
    let mut body = switching_on_a_copy(Vec::new());
    body.local_decls
        .push(LocalDecl::new(ty(TypeKind::Boolean), Span::default()));
    let chained = Local(9);
    let header = &mut body.basic_blocks[LOOP_HEADER].statements;
    header[1] = assign(Place::new(chained), compare(BinOp::Lt, INDEX, LENGTH));
    header.insert(
        2,
        assign(
            Place::new(COMPARED),
            Rvalue::Use(Operand::Copy(Place::new(chained))),
        ),
    );
    assert!(proofs_of(body).is_empty());
}

/// The locals of [`bound_replaced_by_the_length`].
pub(crate) const REPLACED: Local = Local(8);
pub(crate) const SHORTER: Local = Local(9);
pub(crate) const ONE_PAST: Local = Local(10);
/// The block of [`bound_replaced_by_the_length`] reading `list[q]`.
pub(crate) const READ_ONE_PAST: usize = 5;

/// The test loop whose step runs
/// `x = 0; p = Len(spare); if x < L: x = L; if p < x: q = p + 1; e = list[q]`.
/// `x < L` holds of the old `x`; once `x = L`, nothing orders `p + 1`
/// below `L`, and `p` may be `L - 1`.
pub(crate) fn bound_replaced_by_the_length() -> Body {
    let mut body = loop_over_a_list();
    let condition = |body: &mut Body| {
        body.local_decls
            .push(LocalDecl::new(ty(TypeKind::Boolean), Span::default()));
        Local(body.local_decls.len() - 1)
    };
    for _ in 0..3 {
        body.local_decls
            .push(LocalDecl::new(ty(TypeKind::Int), Span::default()));
    }
    let (first, second) = (condition(&mut body), condition(&mut body));
    let switch = |on: Local, taken: usize| {
        terminator(TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(on)),
            targets: vec![(Discriminant::bool_true(), BasicBlock(taken))],
            otherwise: BasicBlock(6),
        })
    };
    let replace = 4;
    let step = &mut body.basic_blocks[LOOP_BODY];
    step.statements = vec![
        assign(Place::new(REPLACED), Rvalue::Use(int(0))),
        assign(Place::new(SHORTER), Rvalue::Len(Place::new(SPARE_LIST))),
        assign(Place::new(first), compare(BinOp::Lt, REPLACED, LENGTH)),
    ];
    step.terminator = Some(switch(first, replace));
    let mut replaced = miri::mir::BasicBlockData::new(Some(switch(second, READ_ONE_PAST)));
    replaced.statements = vec![
        assign(
            Place::new(REPLACED),
            Rvalue::Use(Operand::Copy(Place::new(LENGTH))),
        ),
        assign(Place::new(second), compare(BinOp::Lt, SHORTER, REPLACED)),
    ];
    let mut read = miri::mir::BasicBlockData::new(Some(terminator(TerminatorKind::Goto {
        target: BasicBlock(6),
    })));
    read.statements = vec![
        assign(Place::new(ONE_PAST), stepped(BinOp::Add, SHORTER, 1)),
        assign(Place::new(ELEMENT), element_at(ONE_PAST)),
    ];
    let mut next = miri::mir::BasicBlockData::new(Some(terminator(TerminatorKind::Goto {
        target: BasicBlock(LOOP_HEADER),
    })));
    next.statements = vec![assign(Place::new(INDEX), stepped(BinOp::Add, INDEX, 1))];
    body.basic_blocks.extend([replaced, read, next]);
    body
}

#[test]
fn test_an_order_of_a_local_before_it_takes_the_bounds_value_proves_nothing_after() {
    let proofs = proofs_of(bound_replaced_by_the_length());
    assert!(
        proofs.iter().all(|proof| proof.index != ONE_PAST),
        "{proofs:?}"
    );
}
