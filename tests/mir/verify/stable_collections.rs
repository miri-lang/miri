// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the check that every collection recorded stable over a loop keeps
//! its header there. Each record is forged by hand onto a body it does not
//! hold for, and the check has to say why.

use crate::mir::optimization::stable_collections::{
    borrowing, list_of, loop_over_a_list, loop_over_an_owned_list, program_call,
};
use miri::ast::types::TypeKind;
use miri::error::syntax::Span;
use miri::mir::facts::OwnershipFacts;
use miri::mir::header_reads::stable_collections;
use miri::mir::optimization::stable_collections::record_stable_collections;
use miri::mir::symbol::Symbol;
use miri::mir::verify::verify_stable_collections;
use miri::mir::{
    BasicBlock, BasicBlockData, Body, HeaderRead, Local, LocalDecl, Operand, Place, PlaceElem,
    Rvalue, StableCollection, Statement, StatementKind, Terminator, TerminatorKind,
};

const LIST: Local = Local(1);
const SPARE_LIST: Local = Local(6);
const SPARE: Local = Local(7);

/// A read of `local`'s header over the test loop's blocks.
fn header_read(local: Local) -> Statement {
    Statement {
        kind: StatementKind::ReadHeader(HeaderRead {
            collection: Place::new(local),
            header: BasicBlock(1),
            blocks: vec![BasicBlock(1), BasicBlock(2)],
        }),
        span: Span::default(),
    }
}

/// `body` with a preheader appended reading the list's header over the
/// loop, the entry left as it was.
fn forge_record(mut body: Body) -> Body {
    let preheader = body.basic_blocks.len();
    let mut block = BasicBlockData::new(Some(Terminator::new(
        TerminatorKind::Goto {
            target: BasicBlock(1),
        },
        Span::default(),
    )));
    block.statements.push(header_read(LIST));
    body.basic_blocks.push(block);
    body.basic_blocks[0]
        .terminator
        .as_mut()
        .unwrap()
        .replace_successor(BasicBlock(1), BasicBlock(preheader));
    stable_collections(&body).push(StableCollection {
        local: LIST,
        preheader: BasicBlock(preheader),
        header: BasicBlock(1),
        blocks: vec![BasicBlock(1), BasicBlock(2)],
    });
    body
}

fn messages(body: &Body) -> Vec<String> {
    messages_with(body, &OwnershipFacts::default())
}

fn messages_with(body: &Body, ownership: &OwnershipFacts) -> Vec<String> {
    verify_stable_collections(body, ownership, None)
        .into_iter()
        .map(|violation| violation.message)
        .collect()
}

#[test]
fn test_a_record_the_pass_made_has_no_violation() {
    let mut body = loop_over_a_list();
    assert!(record_stable_collections(
        &mut body,
        &OwnershipFacts::default()
    ));
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
            flags: miri::mir::CallFlags::default(),
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
    let preheader = stable_collections(&body)[0].preheader;
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

/// Two reads of one list's header covering one block leave code generation
/// a choice MIR must make.
#[test]
fn test_two_records_covering_one_block_for_one_list_are_flagged() {
    let mut body = forge_record(loop_over_a_list());
    let preheader = body.basic_blocks.len() - 1;
    body.basic_blocks[preheader]
        .statements
        .push(header_read(LIST));
    let found = messages(&body);
    assert!(
        found
            .iter()
            .any(|message| message.contains("is read once in both")),
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
    let mut body = loop_over_a_list();
    body = forge_record(body);
    let preheader = body.basic_blocks.len() - 1;
    body.basic_blocks[preheader].statements[0] = header_read(Local(2));
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
                projection: vec![PlaceElem::index(Local(2))],
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

/// `found`, which must hold a message containing `fragment`.
fn assert_flagged(body: &Body, fragment: &str) {
    assert_flagged_with(body, &OwnershipFacts::default(), fragment);
}

fn assert_flagged_with(body: &Body, ownership: &OwnershipFacts, fragment: &str) {
    let found = messages_with(body, ownership);
    assert!(
        found.iter().any(|message| message.contains(fragment)),
        "{found:?}"
    );
}

#[test]
fn test_a_record_the_pass_made_over_a_loop_calling_a_function_of_the_program_has_no_violation() {
    let mut body = loop_over_an_owned_list();
    body.basic_blocks[2].terminator = Some(program_call(Vec::new()));
    assert!(
        record_stable_collections(&mut body, &OwnershipFacts::default()),
        "{body}"
    );
    assert_eq!(messages(&body), Vec::<String>::new());
}

#[test]
fn test_a_record_over_a_loop_calling_a_function_handed_a_copy_of_the_list_is_flagged() {
    let mut body = loop_over_an_owned_list();
    body.basic_blocks[0].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(SPARE_LIST),
            Rvalue::Use(Operand::Copy(Place::new(LIST))),
        ),
        span: Span::default(),
    });
    body.basic_blocks[2].terminator =
        Some(program_call(vec![Operand::Copy(Place::new(SPARE_LIST))]));
    assert_flagged(&forge_record(body), "bb2 calls");
}

#[test]
fn test_a_record_over_a_loop_calling_a_function_handed_a_borrow_of_the_list_is_flagged() {
    let mut body = loop_over_an_owned_list();
    let ownership = borrowing(SPARE_LIST, vec![LIST]);
    body.basic_blocks[2].terminator =
        Some(program_call(vec![Operand::Copy(Place::new(SPARE_LIST))]));
    assert_flagged_with(&forge_record(body), &ownership, "bb2 calls");
}

#[test]
fn test_a_record_of_a_borrowed_list_over_a_loop_calling_a_function_is_flagged() {
    let mut body = loop_over_an_owned_list();
    let ownership = borrowing(LIST, vec![SPARE_LIST]);
    body.basic_blocks[2].terminator = Some(program_call(Vec::new()));
    assert_flagged_with(
        &forge_record(body),
        &ownership,
        "holds no reference of its own",
    );
}

#[test]
fn test_a_record_of_a_list_copied_without_a_retain_over_a_loop_calling_a_function_is_flagged() {
    let mut body = loop_over_an_owned_list();
    body.basic_blocks[0].statements[0] = Statement {
        kind: StatementKind::Assign(
            Place::new(LIST),
            Rvalue::Use(Operand::Copy(Place::new(SPARE_LIST))),
        ),
        span: Span::default(),
    };
    body.basic_blocks[2].terminator = Some(program_call(Vec::new()));
    assert_flagged(
        &forge_record(body),
        "bb0 gives it a value it holds no reference to",
    );
}

#[test]
fn test_a_record_over_a_loop_making_a_virtual_call_is_flagged() {
    let mut body = loop_over_an_owned_list();
    body.basic_blocks[2].terminator = Some(Terminator::new(
        TerminatorKind::VirtualCall {
            vtable_slot: 0,
            args: vec![Operand::Copy(Place::new(SPARE_LIST))],
            arg_modes: Vec::new(),
            param_types: Vec::new(),
            destination: Place::new(SPARE),
            target: Some(BasicBlock(1)),
        },
        Span::default(),
    ));
    assert_flagged(&forge_record(body), "bb2 calls through a vtable");
}
