// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the pass that records the element accesses a loop guard proves
//! in bounds. Each hand-built body is the stable-collections test loop
//! `while i < list.length(): x = list[i]; i += 1`, changed in one way.

use crate::mir::optimization::stable_collections::{
    assign, call, int, list_of, loop_over_a_list, statement, INDEX, LENGTH, LIST, LOOP_BODY,
    LOOP_HEADER, SPARE, SPARE_LIST,
};
use miri::ast::types::TypeKind;
use miri::error::syntax::Span;
use miri::mir::optimization::in_bounds::{record_in_bounds, INTEGER_ARITHMETIC_TRAPS_ON_OVERFLOW};
use miri::mir::optimization::stable_collections::record_stable_collections;
use miri::mir::symbol::Symbol;
use miri::mir::{
    BasicBlock, BinOp, Body, ExecutionModel, InBounds, LocalDecl, Operand, Place, Rvalue,
    StatementKind,
};
use miri::pipeline::Pipeline;

/// The in-bounds entries `body` gets once its stable collections are
/// recorded.
pub(crate) fn proofs_of(mut body: Body) -> Vec<InBounds> {
    record_stable_collections(&mut body);
    record_in_bounds(&mut body);
    body.in_bounds
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
