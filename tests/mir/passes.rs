// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A pass over a body keeps what it declares about the control-flow graph:
//! a pass that rewrites the graph records it, and only a pass that declares
//! it rewrites the graph may. The facts recorded about a body follow what
//! each pass declares about them: a rewrite carries the ones it preserves and
//! leaves every other one stale, and no pass starts on a stale fact it reads.

use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::facts::{BodyFacts, Fact, ProgramFacts, StaleFact};
use miri::mir::passes::{
    run_body_pass, run_body_pass_with_facts, run_program_pass_with_facts, BodyPass, PassContract,
};
use miri::mir::Local;
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
    requires: &[],
    records: &[],
    preserves: &[],
};

const READER: BodyPass = BodyPass {
    name: "reader",
    rewrites_cfg: false,
    requires: &[],
    records: &[],
    preserves: &[],
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

/// A rewrite declaring nothing about the ownership facts.
const FORGETFUL_REWRITER: BodyPass = BodyPass {
    name: "forgetful_rewriter",
    rewrites_cfg: true,
    requires: &[],
    records: &[],
    preserves: &[],
};

/// A rewrite declaring the ownership facts still hold after it.
const PRESERVING_REWRITER: BodyPass = BodyPass {
    name: "preserving_rewriter",
    rewrites_cfg: true,
    requires: &[],
    records: &[],
    preserves: &[Fact::Ownership],
};

/// A pass reading the ownership facts.
const OWNERSHIP_READER: BodyPass = BodyPass {
    name: "ownership_reader",
    rewrites_cfg: false,
    requires: &[Fact::Ownership],
    records: &[],
    preserves: &[],
};

/// A rewrite recording the ownership facts afresh.
const RECORDING_REWRITER: BodyPass = BodyPass {
    name: "recording_rewriter",
    rewrites_cfg: true,
    requires: &[],
    records: &[Fact::Ownership],
    preserves: &[],
};

/// Rewrites `body` and records it, recording nothing about its facts.
fn rewrite(body: &mut Body, _: &mut BodyFacts) {
    split_entry(body);
    body.cfg_changed();
}

#[test]
fn a_rewrite_that_does_not_preserve_a_fact_leaves_it_stale() {
    let mut body = returning_body();
    let mut facts = BodyFacts::new(&body);
    let recorded_at = body.generation();
    let run = run_body_pass_with_facts(&FORGETFUL_REWRITER, &mut body, &mut facts, rewrite);
    assert_eq!(run, Ok(()));
    assert_eq!(
        facts.current_ownership(&body),
        Err(StaleFact {
            fact: Fact::Ownership,
            recorded_at: None,
            read_at: body.generation(),
        })
    );
    assert_ne!(body.generation(), recorded_at);
}

#[test]
fn a_pass_reading_a_stale_fact_is_refused_before_it_runs() {
    let mut body = returning_body();
    let mut facts = BodyFacts::new(&body);
    run_body_pass_with_facts(&FORGETFUL_REWRITER, &mut body, &mut facts, rewrite)
        .expect("the rewrite is declared");
    let mut ran = false;
    let run = run_body_pass_with_facts(&OWNERSHIP_READER, &mut body, &mut facts, |_, _| ran = true);
    let Err(PassContract::Stale(pass, stale)) = run else {
        panic!("a pass started on a stale fact: {run:?}");
    };
    assert_eq!((pass, stale.fact), ("ownership_reader", Fact::Ownership));
    assert!(!ran, "the pass ran on a stale fact");
}

#[test]
fn a_rewrite_that_preserves_a_fact_carries_it_to_the_new_generation() {
    let mut body = returning_body();
    let mut facts = BodyFacts::new(&body);
    facts.ownership_mut().owned_params = vec![true];
    run_body_pass_with_facts(&PRESERVING_REWRITER, &mut body, &mut facts, rewrite)
        .expect("the rewrite is declared");
    let ownership = facts
        .current_ownership(&body)
        .expect("the fact was carried");
    assert_eq!(ownership.owned_params, vec![true]);
}

#[test]
fn a_rewrite_that_records_a_fact_leaves_it_current() {
    let mut body = returning_body();
    let mut facts = BodyFacts::new(&body);
    run_body_pass_with_facts(&RECORDING_REWRITER, &mut body, &mut facts, |body, facts| {
        rewrite(body, facts);
        facts
            .ownership_mut()
            .borrowed_locals
            .insert(Local(0), Vec::new());
    })
    .expect("the rewrite is declared");
    let ownership = facts
        .current_ownership(&body)
        .expect("the fact was recorded");
    assert!(ownership.is_borrowed(Local(0)));
}

#[test]
fn a_pass_using_facts_cannot_run_without_them() {
    let mut body = returning_body();
    let run = run_body_pass(&OWNERSHIP_READER, &mut body, |_| ());
    assert_eq!(run, Err(PassContract::RunWithoutFacts("ownership_reader")));
}

#[test]
fn a_rewrite_run_without_facts_leaves_every_fact_stale() {
    let mut body = returning_body();
    let facts = BodyFacts::new(&body);
    run_body_pass(&REWRITER, &mut body, |body| {
        rewrite(body, &mut BodyFacts::new(body))
    })
    .expect("the rewrite is declared");
    assert!(facts.current_ownership(&body).is_err());
}

#[test]
fn program_facts_that_do_not_describe_every_body_are_refused() {
    let mut bodies = vec![("f", returning_body()), ("g", returning_body())];
    let mut facts = ProgramFacts::of(&bodies[..1]);
    let run = run_program_pass_with_facts(&OWNERSHIP_READER, &mut bodies, &mut facts, |_, _| ());
    assert_eq!(
        run,
        Err(PassContract::FactsMisaligned {
            facts: 1,
            bodies: 2
        })
    );
}
