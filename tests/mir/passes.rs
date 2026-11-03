// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A pass over a body keeps what it declares about the control-flow graph:
//! a pass that rewrites the graph records it, and only a pass that declares
//! it rewrites the graph may.

use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::passes::{run_body_pass, BodyPass, PassContract};
use miri::mir::{Body, ExecutionModel, LocalDecl, Terminator, TerminatorKind};

/// `fn f()`: one block that returns.
fn returning_body() -> Body {
    let span = Span::default();
    let mut body = Body::new(0, span, ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(Type::new(TypeKind::Void, span), span));
    let entry = BasicBlockData::new(Some(Terminator::new(TerminatorKind::Return, span)));
    body.basic_blocks.push(entry);
    body
}

/// Puts a block in front of the entry's return, the way a pass inserting a
/// preheader would.
fn split_entry(body: &mut Body) {
    let span = Span::default();
    let tail = BasicBlock(body.basic_blocks.len());
    body.basic_blocks
        .push(BasicBlockData::new(Some(Terminator::new(
            TerminatorKind::Return,
            span,
        ))));
    body.basic_blocks[0].terminator =
        Some(Terminator::new(TerminatorKind::Goto { target: tail }, span));
}

const REWRITER: BodyPass = BodyPass {
    name: "rewriter",
    rewrites_cfg: true,
};

const READER: BodyPass = BodyPass {
    name: "reader",
    rewrites_cfg: false,
};

#[test]
fn a_pass_that_rewrites_the_graph_without_recording_it_is_refused() {
    let mut body = returning_body();
    let fact_generation = body.generation();
    let run = run_body_pass(&REWRITER, &mut body, split_entry);
    assert_eq!(run, Err(PassContract::RewroteWithoutBumping("rewriter")));
    assert_eq!(
        body.generation(),
        fact_generation,
        "a fact recorded before the rewrite still looks current, which is what the check refuses"
    );
}

#[test]
fn a_pass_that_records_a_rewrite_it_does_not_declare_is_refused() {
    let mut body = returning_body();
    let run = run_body_pass(&READER, &mut body, |body| {
        split_entry(body);
        body.cfg_changed();
    });
    assert_eq!(run, Err(PassContract::RewroteUndeclared("reader")));
}

#[test]
fn a_declared_rewrite_that_is_recorded_moves_the_body_to_a_new_generation() {
    let mut body = returning_body();
    let fact_generation = body.generation();
    let run = run_body_pass(&REWRITER, &mut body, |body| {
        split_entry(body);
        body.cfg_changed();
    });
    assert_eq!(run, Ok(()));
    assert_ne!(body.generation(), fact_generation);
}

#[test]
fn a_pass_that_leaves_the_graph_alone_keeps_the_generation() {
    let mut body = returning_body();
    let fact_generation = body.generation();
    assert_eq!(run_body_pass(&READER, &mut body, |_| ()), Ok(()));
    assert_eq!(body.generation(), fact_generation);
}
