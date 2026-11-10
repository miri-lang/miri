// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for how the reference-count verifier reads a drop flag: it follows
//! the flag's value from the constant writes the body makes rather than
//! taking the flag's word for what its binding holds, and it holds every
//! write to the flag to where it may sit.
//!
//! Each broken fixture is the defect a rule exists for, made by removing or
//! changing one write in the shape drop elaboration leaves.

use miri::ast::literal::Literal;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::facts::OwnershipFacts;
use miri::mir::verify::{verify_body, VerificationViolation};
use miri::mir::{
    Body, Constant, Discriminant, EndCause, ExecutionModel, FlagOf, Local, LocalDecl, Operand,
    Place, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};

const BINDING: usize = 1;
const FLAG: usize = 2;

fn span() -> Span {
    Span::new(0, 0)
}

fn stmt(kind: StatementKind) -> Statement {
    Statement { kind, span: span() }
}

fn place(local: usize) -> Place {
    Place::new(Local(local))
}

fn constant(ty: TypeKind, literal: Literal) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: span(),
        ty: Type::new(ty, span()),
        literal,
    }))
}

fn bind() -> Statement {
    let fresh = constant(TypeKind::String, Literal::String("x".to_string()));
    stmt(StatementKind::Assign(place(BINDING), Rvalue::Use(fresh)))
}

fn set(value: bool) -> Statement {
    let value = constant(TypeKind::Boolean, Literal::Boolean(value));
    stmt(StatementKind::Assign(place(FLAG), Rvalue::Use(value)))
}

fn release_and_end(cause: EndCause) -> [Statement; 2] {
    [
        stmt(StatementKind::DecRef(place(BINDING))),
        stmt(StatementKind::StorageDead(place(BINDING), cause)),
    ]
}

fn block(statements: Vec<Statement>, kind: TerminatorKind) -> BasicBlockData {
    BasicBlockData {
        statements,
        terminator: Some(Terminator::new(kind, span())),
        is_cleanup: false,
    }
}

fn goto(target: usize) -> TerminatorKind {
    TerminatorKind::Goto {
        target: BasicBlock(target),
    }
}

/// The shape drop elaboration leaves for a string `_1` consumed on one arm:
/// its flag `_2` cleared on entry, set where `_1` is bound, cleared where
/// either end of it runs, and tested before the release where its scope
/// ends. `edit` changes the blocks first.
fn flag_guarded(edit: impl FnOnce(&mut Vec<BasicBlockData>)) -> Body {
    let mut consumed = release_and_end(EndCause::Consumed).to_vec();
    consumed.push(set(false));
    let mut released = release_and_end(EndCause::Scope).to_vec();
    released.push(set(false));
    let mut blocks = vec![
        block(
            vec![set(false), bind(), set(true)],
            TerminatorKind::SwitchInt {
                discr: constant(TypeKind::Boolean, Literal::Boolean(true)),
                targets: vec![(Discriminant::bool_true(), BasicBlock(1))],
                otherwise: BasicBlock(2),
            },
        ),
        block(consumed, goto(3)),
        block(vec![], goto(3)),
        block(
            vec![],
            TerminatorKind::SwitchInt {
                discr: Operand::Copy(place(FLAG)),
                targets: vec![(Discriminant::bool_false(), BasicBlock(5))],
                otherwise: BasicBlock(4),
            },
        ),
        block(released, goto(5)),
        block(vec![], TerminatorKind::Return),
    ];
    edit(&mut blocks);
    let mut body = Body::new(0, span(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(Type::new(TypeKind::Void, span()), span()));
    body.new_local(LocalDecl::new(Type::new(TypeKind::String, span()), span()));
    let mut flag = LocalDecl::new(Type::new(TypeKind::Boolean, span()), span());
    flag.flag_of = Some(FlagOf::Drop(Local(BINDING)));
    body.new_local(flag);
    body.basic_blocks.extend(blocks);
    body
}

fn findings(body: &Body) -> String {
    let violations = verify_body(body, &OwnershipFacts::default());
    (violations.iter())
        .map(VerificationViolation::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

#[test]
fn a_release_behind_a_flag_kept_in_step_is_accepted() {
    assert_eq!(findings(&flag_guarded(|_| {})), "");
}

#[test]
fn a_flag_never_set_where_its_binding_is_bound_leaves_the_binding_leaked() {
    let body = flag_guarded(|blocks| {
        blocks[0].statements.pop();
    });
    let found = findings(&body);
    assert!(found.contains("still owns 1 reference(s)"), "{found}");
}

#[test]
fn a_flag_written_with_anything_but_a_constant_is_refused() {
    let body = flag_guarded(|blocks| {
        let copied = Rvalue::Use(Operand::Copy(place(FLAG)));
        blocks[0].statements[2] = stmt(StatementKind::Assign(place(FLAG), copied));
    });
    let found = findings(&body);
    assert!(found.contains("is written only with a constant"), "{found}");
}

#[test]
fn a_flag_set_where_its_binding_was_not_just_bound_is_refused() {
    let body = flag_guarded(|blocks| blocks[2].statements.push(set(true)));
    let found = findings(&body);
    assert!(found.contains("is set only right after"), "{found}");
}

#[test]
fn a_flag_cleared_where_its_binding_still_holds_a_value_is_refused() {
    let body = flag_guarded(|blocks| blocks[2].statements.push(set(false)));
    let found = findings(&body);
    assert!(found.contains("is cleared only right after"), "{found}");
}
