// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rule that a body never writes a parameter whose value its
//! caller still holds.
//!
//! The fixture is the copy-on-write a writing call on a parameter used to
//! lower to: the parameter handed to the list's copy-on-write entry point and
//! the result stored back into it.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::BasicBlockData;
use miri::mir::verify::verify_parameter_writes;
use miri::mir::{
    Body, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue, Statement, StatementKind,
    Terminator, TerminatorKind,
};

fn span() -> Span {
    Span::new(0, 0)
}

fn list_ty() -> Type {
    let int = Expression::new(
        0,
        ExpressionKind::Type(Box::new(Type::new(TypeKind::Int, span())), false),
        span(),
    );
    Type::new(
        TypeKind::Custom("List".to_string(), Some(vec![int])),
        span(),
    )
}

fn block(statements: Vec<Statement>, kind: TerminatorKind) -> BasicBlockData {
    BasicBlockData {
        statements,
        terminator: Some(Terminator { kind, span: span() }),
        is_cleanup: false,
    }
}

/// `fn(_1 [int])` whose only block hands `args` to the runtime entry point
/// `name` and returns.
fn handing_to_runtime(name: &str, args: Vec<Operand>) -> Body {
    let mut body = Body::new(1, span(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(Type::new(TypeKind::Void, span()), span()));
    body.new_local(LocalDecl::new(list_ty(), span()));
    body.new_local(LocalDecl::new(Type::new(TypeKind::Void, span()), span()));
    body.out_params = vec![false];
    let call = TerminatorKind::Call {
        func: Operand::runtime(name, span()),
        args,
        arg_modes: Vec::new(),
        arg_handles: Vec::new(),
        destination: Place::new(Local(2)),
        target: Some(miri::mir::BasicBlock(1)),
    };
    body.basic_blocks = vec![
        block(Vec::new(), call),
        block(Vec::new(), TerminatorKind::Return),
    ];
    body
}

fn int_constant() -> Operand {
    Operand::Constant(Box::new(miri::mir::Constant {
        span: span(),
        ty: Type::new(TypeKind::Int, span()),
        literal: miri::ast::literal::Literal::Integer(miri::ast::literal::IntegerLiteral::I64(7)),
    }))
}

/// `fn(_1 [int])` copying `_1` on write in place: `_2 = cow(move _1); _1 = move _2`.
fn copied_in_place() -> Body {
    let mut body = Body::new(1, span(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(Type::new(TypeKind::Void, span()), span()));
    body.new_local(LocalDecl::new(list_ty(), span()));
    body.new_local(LocalDecl::new(list_ty(), span()));
    body.out_params = vec![false];
    let cow = TerminatorKind::Call {
        func: Operand::runtime("miri_rt_list_cow", span()),
        args: vec![Operand::Move(Place::new(Local(1)))],
        arg_modes: Vec::new(),
        arg_handles: Vec::new(),
        destination: Place::new(Local(2)),
        target: Some(miri::mir::BasicBlock(1)),
    };
    let store = Statement {
        kind: StatementKind::Assign(
            Place::new(Local(1)),
            Rvalue::Use(Operand::Move(Place::new(Local(2)))),
        ),
        span: span(),
    };
    body.basic_blocks = vec![
        block(Vec::new(), cow),
        block(vec![store], TerminatorKind::Return),
    ];
    body
}

#[test]
fn a_parameter_copied_on_write_in_place_is_flagged_once() {
    let violations = verify_parameter_writes(&copied_in_place());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0].local, Local(1));
    assert!(violations[0].message.contains("caller still holds"));
}

#[test]
fn a_receiver_copied_on_write_in_place_is_flagged_once() {
    let mut body = copied_in_place();
    body.receiver = Some(Local(1));
    let violations = verify_parameter_writes(&body);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0].local, Local(1));
    assert!(violations[0].message.contains("receiver"));
}

#[test]
fn a_receiver_asked_whether_it_is_shared_is_flagged() {
    let mut body = handing_to_runtime(
        "miri_rt_rc_is_shared",
        vec![Operand::Copy(Place::new(Local(1)))],
    );
    body.receiver = Some(Local(1));
    assert_eq!(verify_parameter_writes(&body).len(), 1);
}

#[test]
fn a_receiver_assigned_whole_is_flagged() {
    let mut body = handing_to_runtime(
        "miri_rt_list_len",
        vec![Operand::Copy(Place::new(Local(1)))],
    );
    body.receiver = Some(Local(1));
    body.basic_blocks[1].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(1)),
            Rvalue::Use(Operand::Copy(Place::new(Local(1)))),
        ),
        span: span(),
    });
    assert_eq!(verify_parameter_writes(&body).len(), 1);
}

#[test]
fn a_receiver_checked_unique_and_written_in_place_raises_nothing() {
    for (name, args) in [
        (
            "miri_rt_receiver_unique",
            vec![Operand::Copy(Place::new(Local(1)))],
        ),
        (
            "miri_rt_list_push",
            vec![Operand::Move(Place::new(Local(1))), int_constant()],
        ),
    ] {
        let mut body = handing_to_runtime(name, args);
        body.receiver = Some(Local(1));
        assert!(verify_parameter_writes(&body).is_empty(), "{name}");
    }
}

#[test]
fn a_field_of_a_receiver_copied_on_write_raises_nothing() {
    let mut field = Place::new(Local(1));
    field.projection.push(miri::mir::PlaceElem::Field(0));
    let mut body = handing_to_runtime("miri_rt_rc_is_shared", vec![Operand::Copy(field)]);
    body.receiver = Some(Local(1));
    assert!(verify_parameter_writes(&body).is_empty());
}

#[test]
fn an_out_parameter_copied_on_write_in_place_raises_nothing() {
    let mut body = copied_in_place();
    body.out_params = vec![true];
    assert!(verify_parameter_writes(&body).is_empty());
}

#[test]
fn a_parameter_the_body_owns_copied_on_write_in_place_raises_nothing() {
    let mut body = copied_in_place();
    body.owned_params = vec![true];
    assert!(verify_parameter_writes(&body).is_empty());
}

#[test]
fn a_parameter_a_runtime_writer_is_handed_is_flagged() {
    let receiver = Operand::Move(Place::new(Local(1)));
    let body = handing_to_runtime("miri_rt_list_push", vec![receiver, int_constant()]);
    let violations = verify_parameter_writes(&body);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0].local, Local(1));
}

#[test]
fn a_projection_of_a_parameter_a_runtime_writer_is_handed_is_flagged() {
    let mut element = Place::new(Local(1));
    element
        .projection
        .push(miri::mir::PlaceElem::Index(Local(2)));
    let body = handing_to_runtime(
        "miri_rt_list_push",
        vec![Operand::Copy(element), int_constant()],
    );
    assert_eq!(verify_parameter_writes(&body).len(), 1);
}

#[test]
fn a_parameter_a_runtime_entry_only_reads_raises_nothing() {
    let body = handing_to_runtime(
        "miri_rt_list_len",
        vec![Operand::Copy(Place::new(Local(1)))],
    );
    assert!(verify_parameter_writes(&body).is_empty());
}

#[test]
fn a_parameter_stored_as_an_element_raises_nothing() {
    let body = handing_to_runtime(
        "miri_rt_list_push",
        vec![int_constant(), Operand::Copy(Place::new(Local(1)))],
    );
    assert!(verify_parameter_writes(&body).is_empty());
}

/// `String.join` and `List(array)` hand their collection parameter to an
/// entry point that reads every element and writes none of them.
#[test]
fn a_parameter_joined_or_copied_into_a_list_raises_nothing() {
    for (name, args) in [
        (
            "miri_rt_string_join",
            vec![int_constant(), Operand::Copy(Place::new(Local(1)))],
        ),
        (
            "miri_rt_list_new_from_managed_array",
            vec![
                Operand::Move(Place::new(Local(1))),
                int_constant(),
                int_constant(),
            ],
        ),
    ] {
        let body = handing_to_runtime(name, args);
        assert!(verify_parameter_writes(&body).is_empty(), "{name}");
    }
}
