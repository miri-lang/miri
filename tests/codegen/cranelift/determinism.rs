// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for build reproducibility (deterministic code generation).
//!
//! A program compiled multiple times from identical source must produce
//! byte-identical object files. This test verifies that the iteration order of
//! the string-literal pool does not permute the emitted string constants.

use miri::ast::literal::Literal;
use miri::ast::types::{Type, TypeKind};
use miri::codegen::cranelift::{CraneliftBackend, CraneliftOptions};
use miri::codegen::Backend;
use miri::error::syntax::Span;
use miri::mir::{
    BasicBlockData, Body, Constant, ExecutionModel, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};

fn span() -> Span {
    Span::new(0, 0)
}

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

/// Assign one string constant to the local at `index`.
fn assign_string(index: usize, text: &str) -> Statement {
    Statement {
        kind: StatementKind::Assign(
            Place {
                local: miri::mir::Local(index),
                projection: vec![],
            },
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span: span(),
                ty: ty(TypeKind::String),
                literal: Literal::String(text.to_string()),
            }))),
        ),
        span: span(),
    }
}

/// Build a body holding several distinct string constants.
///
/// More than one literal is essential: a single-literal pool cannot permute, so
/// a one-literal body would pass whatever the pool's iteration order is.
fn body_with_string_constants() -> Body {
    const LITERALS: [&str; 3] = ["alpha", "bravo", "charlie"];

    let mut body = Body::new(0, span(), ExecutionModel::Cpu);
    body.local_decls
        .push(LocalDecl::new(ty(TypeKind::Void), span()));

    let mut block = BasicBlockData::new(None);
    for (offset, text) in LITERALS.iter().enumerate() {
        body.local_decls
            .push(LocalDecl::new(ty(TypeKind::String), span()));
        block.statements.push(assign_string(offset + 1, text));
    }
    block.terminator = Some(Terminator::new(TerminatorKind::Return, span()));
    body.basic_blocks.push(block);

    body
}

/// Report where two object files first disagree, for a readable failure.
///
/// The byte lengths are equal in the failure this test exists to catch, so the
/// offset is the only detail that identifies the drift.
fn first_difference(left: &[u8], right: &[u8]) -> Option<usize> {
    if left.len() != right.len() {
        return Some(left.len().min(right.len()));
    }
    left.iter().zip(right.iter()).position(|(a, b)| a != b)
}

#[test]
fn cranelift_emits_identical_object_bytes_across_compilations() {
    // Rust re-seeds each hash map it builds, so repeated compilations within one
    // process exercise different iteration orders. Several runs therefore make an
    // ordering bug reliably visible rather than leaving it to chance.
    const RUNS: usize = 5;

    let body = body_with_string_constants();
    let backend = CraneliftBackend::new().expect("failed to create the Cranelift backend");
    let options = CraneliftOptions::default();

    let mut artifacts = Vec::with_capacity(RUNS);
    for run in 0..RUNS {
        let compiled = backend
            .compile(&[("test_func", &body)], &options)
            .unwrap_or_else(|error| panic!("compilation run {} failed: {}", run, error));
        artifacts.push(compiled.bytes);
    }

    let first = &artifacts[0];
    for (run, artifact) in artifacts.iter().enumerate().skip(1) {
        if let Some(offset) = first_difference(first, artifact) {
            panic!(
                "compilation run {} differs from run 0 at byte {} ({} bytes vs {} bytes)",
                run,
                offset,
                artifact.len(),
                first.len()
            );
        }
    }
}
