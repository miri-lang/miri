// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Shared helpers for the MIR tests.
//!
//! # Which assertion style to use
//!
//! - [`mir_snapshot_test`] — the *shape* of the lowered body is the claim: how
//!   many basic blocks exist, in what order they are emitted, which temps they
//!   use, how the terminators wire them together. It compares the whole
//!   pretty-printed body, so it is brittle on purpose: any lowering reorder
//!   breaks it, which is exactly the regression these tests exist to catch.
//!   When lowering legitimately changes, re-read the new shape and update the
//!   expectation — do not weaken the test into a substring check.
//! - [`mir_snapshot_contains_test`] — one targeted property of the printed body
//!   (a specific statement, cast, intrinsic, or terminator is present) where the
//!   surrounding shape is not part of the claim. Use it so the test does not
//!   accidentally re-assert unrelated lowering.
//! - A direct assertion over the [`Body`] returned by [`mir_lower_code`] — the
//!   property is not visible in the printed form (storage classes, launch
//!   arguments, declaration order) or is only expressible by walking the body.
//!
//! # What belongs in this file
//!
//! Lowering, snapshot comparison, and query primitives that walk a [`Body`].
//! Assertions belong in the tests, where the expectation is visible to the
//! reader: a helper that pairs [`mir_lower_code`] with a single `assert!` only
//! hides the expectation behind a name. A multi-assert routine used by exactly
//! one test file lives in that file, not here.

use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::statement::StatementKind as AstStatementKind;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::lowering::lower_function;
use miri::mir::{
    Body, Constant, GpuIntrinsic, LocalDecl, Operand, Rvalue, StatementKind, Terminator,
    TerminatorKind,
};
use miri::pipeline::Pipeline;

pub fn mir_lower_code(source: &str) -> Body {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("Frontend failed");

    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|stmt| {
            if let AstStatementKind::FunctionDeclaration(func) = &stmt.node {
                func.name == "main"
            } else {
                false
            }
        })
        .or_else(|| {
            result
                .ast
                .body
                .iter()
                .find(|stmt| matches!(stmt.node, AstStatementKind::FunctionDeclaration(..)))
        })
        .expect("No function declaration found in source");

    lower_function(func_stmt, &result.type_checker, false, false)
        .expect("Lowering failed")
        .0
}

/// Normalize MIR output for comparison by trimming lines and removing empty lines.
fn normalize_mir_output(output: &str) -> String {
    output
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Snapshot test for MIR lowering.
/// Compares the actual MIR output against expected output.
/// Both are normalized (trimmed, empty lines removed) before comparison.
pub fn mir_snapshot_test(source: &str, expected_mir: &str) {
    let body = mir_lower_code(source);
    let actual = format!("{}", body);
    let actual_normalized = normalize_mir_output(&actual);
    let expected_normalized = normalize_mir_output(expected_mir);

    if actual_normalized != expected_normalized {
        panic!(
            "\n\nMIR snapshot mismatch!\n\n\
             === SOURCE ===\n{}\n\n\
             === EXPECTED ===\n{}\n\n\
             === ACTUAL ===\n{}\n",
            source.trim(),
            expected_normalized,
            actual_normalized
        );
    }
}

/// Snapshot test that only checks if the actual MIR contains the expected substrings.
/// Useful for partial validation when full MIR is too verbose.
pub fn mir_snapshot_contains_test(source: &str, expected_fragments: &[&str]) {
    let body = mir_lower_code(source);
    let actual = format!("{}", body);

    for fragment in expected_fragments {
        assert!(
            actual.contains(fragment),
            "\n\nMIR missing expected fragment!\n\n\
             === SOURCE ===\n{}\n\n\
             === MISSING FRAGMENT ===\n{}\n\n\
             === ACTUAL MIR ===\n{}\n",
            source.trim(),
            fragment,
            actual
        );
    }
}

/// Checks that a declaration reaches MIR lowering without a frontend error.
///
/// Class and trait declarations lower no function body of their own, so
/// `tests/mir/class.rs` can only assert that they pass the frontend; the helper
/// makes that limit explicit at every call site.
pub fn mir_frontend_succeeds(source: &str) {
    let pipeline = Pipeline::new();
    pipeline.frontend(source).expect("Frontend should succeed");
}

pub fn local_decl<'a>(body: &'a Body, name: &str) -> Option<&'a LocalDecl> {
    body.local_decls
        .iter()
        .find(|d| d.name.as_deref() == Some(name))
}

pub fn find_local_idx(body: &Body, name: &str) -> Option<usize> {
    body.local_decls
        .iter()
        .position(|d| d.name.as_deref() == Some(name))
}

pub fn has_local(body: &Body, name: &str) -> bool {
    local_decl(body, name).is_some()
}

pub fn count_assignments(body: &Body, block_idx: usize) -> usize {
    body.basic_blocks[block_idx]
        .statements
        .iter()
        .filter(|s| matches!(&s.kind, StatementKind::Assign(..)))
        .count()
}

pub fn get_assignment_order(body: &Body, block_idx: usize) -> Vec<usize> {
    body.basic_blocks[block_idx]
        .statements
        .iter()
        .filter_map(|stmt| {
            if let StatementKind::Assign(place, _) = &stmt.kind {
                Some(place.local.0)
            } else {
                None
            }
        })
        .collect()
}

pub fn count_assignments_to(body: &Body, block_idx: usize, local_idx: usize) -> usize {
    body.basic_blocks[block_idx]
        .statements
        .iter()
        .filter(|s| {
            if let StatementKind::Assign(place, _) = &s.kind {
                place.local.0 == local_idx
            } else {
                false
            }
        })
        .count()
}

pub fn terminator_of(body: &Body, block_idx: usize) -> Option<&Terminator> {
    body.basic_blocks[block_idx].terminator.as_ref()
}

pub fn last_terminator(body: &Body) -> &Terminator {
    body.basic_blocks
        .last()
        .expect("No basic blocks")
        .terminator
        .as_ref()
        .expect("No terminator")
}

pub fn has_gpu_launch(body: &Body) -> bool {
    body.basic_blocks.iter().any(|bb| {
        bb.terminator
            .as_ref()
            .is_some_and(|t| matches!(t.kind, TerminatorKind::GpuLaunch { .. }))
    })
}

/// Buffer-argument count and per-argument read-only flags of the first
/// `GpuLaunch` terminator, or `None` when the body launches no kernel.
pub fn gpu_launch_buffer_args(body: &Body) -> Option<(usize, Vec<bool>)> {
    body.basic_blocks.iter().find_map(|bb| {
        if let Some(TerminatorKind::GpuLaunch { launch_args, .. }) =
            bb.terminator.as_ref().map(|t| &t.kind)
        {
            return Some((launch_args.len(), launch_args.arg_read_only().to_vec()));
        }
        None
    })
}

/// Whether the body assigns the given GPU intrinsic, matching the dimension of
/// the thread/block/grid intrinsics rather than only their variant.
pub fn has_gpu_intrinsic(body: &Body, expected: &GpuIntrinsic) -> bool {
    body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let StatementKind::Assign(_, Rvalue::GpuIntrinsic(intrinsic)) = &stmt.kind {
                gpu_intrinsics_match(intrinsic, expected)
            } else {
                false
            }
        })
    })
}

fn gpu_intrinsics_match(actual: &GpuIntrinsic, expected: &GpuIntrinsic) -> bool {
    if std::mem::discriminant(actual) != std::mem::discriminant(expected) {
        return false;
    }
    match (actual, expected) {
        (GpuIntrinsic::ThreadIdx(a), GpuIntrinsic::ThreadIdx(b)) => a == b,
        (GpuIntrinsic::BlockIdx(a), GpuIntrinsic::BlockIdx(b)) => a == b,
        (GpuIntrinsic::BlockDim(a), GpuIntrinsic::BlockDim(b)) => a == b,
        (GpuIntrinsic::GridDim(a), GpuIntrinsic::GridDim(b)) => a == b,
        _ => true,
    }
}

pub fn make_int_const(val: i32) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: Span::default(),
        ty: Type::new(TypeKind::Int, Span::default()),
        literal: Literal::Integer(IntegerLiteral::I32(val)),
    }))
}

pub fn make_string_const(val: &str) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: Span::default(),
        ty: Type::new(TypeKind::String, Span::default()),
        literal: Literal::String(val.to_string()),
    }))
}
