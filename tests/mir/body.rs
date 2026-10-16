// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::error::syntax::Span;
use miri::mir::{
    ArgMode, BasicBlock, BasicBlockData, Body, ExecutionModel, Local, Operand, Place, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
};

fn statement(kind: StatementKind) -> Statement {
    Statement {
        kind,
        span: Span::default(),
    }
}

#[test]
fn test_written_place_is_an_assignment_destination_only() {
    let target = Place::new(Local(1));
    let copied = Rvalue::Use(Operand::Copy(Place::new(Local(2))));
    for write in [
        StatementKind::Assign(target.clone(), copied.clone()),
        StatementKind::Reassign(target.clone(), copied),
    ] {
        assert_eq!(statement(write).written_place(), Some(&target));
    }
    for other in [
        StatementKind::IncRef(target.clone()),
        StatementKind::DecRef(target.clone()),
        StatementKind::StorageDead(target),
        StatementKind::Nop,
    ] {
        assert_eq!(statement(other).written_place(), None);
    }
}

#[test]
fn test_written_locals_counts_statement_and_terminator_writes() {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    let mut block = BasicBlockData::new(Some(Terminator::new(
        TerminatorKind::Call {
            func: Operand::runtime("callee", Span::default()),
            args: vec![
                Operand::Copy(Place::new(Local(3))),
                Operand::Copy(Place::new(Local(4))),
            ],
            arg_modes: vec![ArgMode::Borrowed, ArgMode::Out],
            arg_handles: Vec::new(),
            destination: Place::new(Local(2)),
            target: Some(BasicBlock(0)),
        },
        Span::default(),
    )));
    block.statements.push(statement(StatementKind::Assign(
        Place::new(Local(1)),
        Rvalue::Use(Operand::Copy(Place::new(Local(5)))),
    )));
    body.basic_blocks.push(block);
    let written = body.written_locals();
    let mut locals: Vec<usize> = written.iter().map(|local| local.0).collect();
    locals.sort_unstable();
    assert_eq!(locals, vec![1, 2, 4]);
}
