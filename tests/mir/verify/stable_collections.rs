// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the check that every collection recorded stable over a loop keeps
//! its header there. Each record is forged by hand onto a body it does not
//! hold for, and the check has to say why.

use crate::mir::optimization::stable_collections::{list_of, loop_over_a_list};
use miri::ast::types::TypeKind;
use miri::error::syntax::Span;
use miri::mir::optimization::stable_collections::record_stable_collections;
use miri::mir::symbol::Symbol;
use miri::mir::verify::verify_stable_collections;
use miri::mir::{
    BasicBlock, BasicBlockData, Body, Local, LocalDecl, Operand, Place, PlaceElem, Rvalue,
    StableCollection, Statement, StatementKind, Terminator, TerminatorKind,
};

const LIST: Local = Local(1);
const SPARE_LIST: Local = Local(6);
const SPARE: Local = Local(7);

/// `body` with a bare preheader appended and a record of the list over the
/// loop, the entry left as it was.
fn forge_record(mut body: Body) -> Body {
    let preheader = body.basic_blocks.len();
    body.basic_blocks
        .push(BasicBlockData::new(Some(Terminator::new(
            TerminatorKind::Goto {
                target: BasicBlock(1),
            },
            Span::default(),
        ))));
    body.basic_blocks[0]
        .terminator
        .as_mut()
        .unwrap()
        .replace_successor(BasicBlock(1), BasicBlock(preheader));
    body.stable_collections.push(StableCollection {
        local: LIST,
        preheader: BasicBlock(preheader),
        header: BasicBlock(1),
        blocks: vec![BasicBlock(1), BasicBlock(2)],
    });
    body
}

fn messages(body: &Body) -> Vec<String> {
    verify_stable_collections(body)
        .into_iter()
        .map(|violation| violation.message)
        .collect()
}

#[test]
fn test_a_record_the_pass_made_has_no_violation() {
    let mut body = loop_over_a_list();
    assert!(record_stable_collections(&mut body));
    assert_eq!(messages(&body), Vec::<String>::new());
}

#[test]
fn test_a_record_over_a_loop_calling_a_function_of_the_program_is_flagged() {
    let mut body = loop_over_a_list();
    body.basic_blocks[2].terminator = Some(Terminator::new(
        TerminatorKind::Call {
            func: Operand::function(Symbol::entry(), Span::default()),
            args: Vec::new(),
            arg_modes: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(SPARE),
            target: Some(BasicBlock(1)),
        },
        Span::default(),
    ));
    let found = messages(&forge_record(body));
    assert!(
        found.iter().any(|message| message.contains("bb2 calls")),
        "{found:?}"
    );
}

#[test]
fn test_a_record_whose_preheader_does_not_dominate_the_loop_is_flagged() {
    let mut body = forge_record(loop_over_a_list());
    let preheader = body.stable_collections[0].preheader;
    body.basic_blocks[0]
        .terminator
        .as_mut()
        .unwrap()
        .replace_successor(preheader, BasicBlock(1));
    let found = messages(&body);
    assert!(
        found
            .iter()
            .any(|message| message.contains("does not dominate bb1, which is entered from bb0")),
        "{found:?}"
    );
}

#[test]
fn test_a_record_over_a_loop_rebinding_the_list_is_flagged() {
    let mut body = loop_over_a_list();
    body.basic_blocks[2].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(LIST),
            Rvalue::Use(Operand::Move(Place::new(SPARE_LIST))),
        ),
        span: Span::default(),
    });
    let found = messages(&forge_record(body));
    assert!(
        found
            .iter()
            .any(|message| message.contains("bb2 gives it another value")),
        "{found:?}"
    );
}

#[test]
fn test_a_record_of_a_local_that_holds_no_collection_is_flagged() {
    let mut body = forge_record(loop_over_a_list());
    body.stable_collections[0].local = Local(2);
    let found = messages(&body);
    assert!(
        found
            .iter()
            .any(|message| message.contains("which has no element header")),
        "{found:?}"
    );
}

#[test]
fn test_a_record_of_a_borrowed_list_over_a_loop_storing_a_list_elsewhere_is_flagged() {
    let mut body = loop_over_a_list();
    let rows = body.new_local(LocalDecl::new(
        list_of(list_of(TypeKind::Int).kind),
        Span::default(),
    ));
    body.basic_blocks[2].statements.push(Statement {
        kind: StatementKind::Assign(
            Place {
                local: rows,
                projection: vec![PlaceElem::Index(Local(2))],
            },
            Rvalue::Use(Operand::Move(Place::new(SPARE_LIST))),
        ),
        span: Span::default(),
    });
    let found = messages(&forge_record(body));
    assert!(
        found
            .iter()
            .any(|message| message.contains("may release the list it borrows")),
        "{found:?}"
    );
}
