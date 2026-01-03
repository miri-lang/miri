// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use miri::ast::statement::StatementKind;
use miri::mir::lowering::lower_function;
use miri::mir::{Body, StatementKind as MirStatementKind, TerminatorKind};
use miri::pipeline::Pipeline;

pub fn lower_code(source: &str) -> Body {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("Frontend failed");

    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|stmt| matches!(stmt.node, StatementKind::FunctionDeclaration(..)))
        .expect("No function declaration found in source");

    lower_function(func_stmt, &result.type_checker).expect("Lowering failed")
}

pub fn expect_assignment(stmt: &miri::mir::Statement) -> (&miri::mir::Place, &miri::mir::Rvalue) {
    match &stmt.kind {
        MirStatementKind::Assign(place, rvalue) => (place, rvalue),
        _ => panic!("Expected Assign statement, got {:?}", stmt.kind),
    }
}

/// Find the index of a local variable by name.
pub fn find_local_idx(body: &Body, name: &str) -> Option<usize> {
    body.local_decls
        .iter()
        .position(|d| d.name.as_deref() == Some(name))
}

/// Check if a local variable with the given name exists.
pub fn has_local(body: &Body, name: &str) -> bool {
    body.local_decls
        .iter()
        .any(|d| d.name.as_deref() == Some(name))
}

/// Count the number of locals with a given name (useful for shadowing tests).
pub fn count_locals_named(body: &Body, name: &str) -> usize {
    body.local_decls
        .iter()
        .filter(|d| d.name.as_deref() == Some(name))
        .count()
}

/// Count the number of assignment statements in a basic block.
pub fn count_assignments(body: &Body, block_idx: usize) -> usize {
    body.basic_blocks[block_idx]
        .statements
        .iter()
        .filter(|s| matches!(&s.kind, MirStatementKind::Assign(..)))
        .count()
}

/// Get the order of assignments by local index in a basic block.
pub fn get_assignment_order(body: &Body, block_idx: usize) -> Vec<usize> {
    body.basic_blocks[block_idx]
        .statements
        .iter()
        .filter_map(|stmt| {
            if let MirStatementKind::Assign(place, _) = &stmt.kind {
                Some(place.local.0)
            } else {
                None
            }
        })
        .collect()
}

/// Count assignments to a specific local by index.
pub fn count_assignments_to(body: &Body, block_idx: usize, local_idx: usize) -> usize {
    body.basic_blocks[block_idx]
        .statements
        .iter()
        .filter(|s| {
            if let MirStatementKind::Assign(place, _) = &s.kind {
                place.local.0 == local_idx
            } else {
                false
            }
        })
        .count()
}

/// Assert that all expected locals exist in the lowered MIR body.
pub fn assert_locals(source: &str, expected_locals: &[&str]) {
    let body = lower_code(source);
    for name in expected_locals {
        assert!(has_local(&body, name), "Expected local '{}' to exist", name);
    }
}

/// Assert that the last basic block has the expected terminator kind.
pub fn assert_terminator(source: &str, expected: TerminatorKind) {
    let body = lower_code(source);
    let last_block = body.basic_blocks.last().expect("No basic blocks");
    let term = last_block.terminator.as_ref().expect("No terminator");
    assert!(
        std::mem::discriminant(&term.kind) == std::mem::discriminant(&expected),
        "Expected {:?} terminator, got {:?}",
        expected,
        term.kind
    );
}
