// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An optional moved from one place into another keeps its bits, so the two
//! places must be laid out alike: a nullable payload pointer stored into a slot
//! read as a box would be opened as one. The MIR verifier refuses such a flow;
//! the backend refuses it again rather than guess, and refuses an optional
//! whose payload it cannot lay out at all — one at the error type, or at a
//! type parameter the body leaves no room for.

use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeDeclarationKind, TypeKind};
use miri::codegen::backend::Backend;
use miri::codegen::cranelift::{CraneliftBackend, CraneliftOptions};
use miri::error::syntax::Span;
use miri::mir::type_facts::{ClassDefinition, TypeDefinition, TypeFacts};
use miri::mir::{
    AggregateKind, BasicBlockData, Body, Constant, ExecutionModel, Local, LocalDecl, Operand,
    Place, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn optional(kind: TypeKind) -> TypeKind {
    TypeKind::Option(Box::new(ty(kind)))
}

fn node() -> TypeKind {
    TypeKind::Custom("Node".to_string(), None)
}

fn parameter() -> TypeKind {
    TypeKind::Generic("T".to_string(), None, TypeDeclarationKind::None)
}

/// A program defining one class, `Node`.
fn facts(body: &Body) -> TypeFacts {
    let definitions = HashMap::from([(
        "Node".to_string(),
        TypeDefinition::Class(ClassDefinition {
            name: "Node".to_string(),
            generics: None,
            base_class: None,
            base_class_args: None,
            traits: Vec::new(),
            trait_args: HashMap::new(),
            fields: Vec::new(),
            methods: BTreeMap::new(),
            module: String::new(),
            is_abstract: false,
            runtime_settled_methods: BTreeSet::new(),
        }),
    )]);
    TypeFacts::new(definitions, HashMap::new(), Default::default(), [body])
        .unwrap_or_else(|error| panic!("the facts settle: {error:?}"))
}

/// A body with `_1` at `source` and `_2` at `target`, assigning `rvalue` to
/// `_2`.
fn body_assigning(source: TypeKind, target: TypeKind, rvalue: Rvalue) -> Body {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ty(TypeKind::Void), Span::default()));
    body.new_local(LocalDecl::new(ty(source), Span::default()));
    body.new_local(LocalDecl::new(ty(target), Span::default()));
    let mut block = BasicBlockData::new(None);
    block.statements.push(Statement {
        kind: StatementKind::Assign(Place::new(Local(2)), rvalue),
        span: Span::default(),
    });
    block.terminator = Some(Terminator::new(TerminatorKind::Return, Span::default()));
    body.basic_blocks.push(block);
    body
}

fn copy_of_source() -> Operand {
    Operand::Copy(Place::new(Local(1)))
}

fn compile(body: &Body) -> Result<(), String> {
    let mut backend = CraneliftBackend::new().map_err(|e| e.to_string())?;
    backend.set_type_facts(facts(body));
    backend
        .compile(&[("flow", body)], &CraneliftOptions::default())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn assert_refused(body: &Body, fragment: &str) {
    match compile(body) {
        Ok(()) => panic!("the backend compiled a flow it cannot lay out"),
        Err(error) => assert!(error.contains(fragment), "unexpected error: {error}"),
    }
}

#[test]
fn a_use_between_optionals_of_one_layout_compiles() {
    let body = body_assigning(
        optional(node()),
        optional(node()),
        Rvalue::Use(copy_of_source()),
    );
    compile(&body).unwrap_or_else(|error| panic!("one layout on both sides: {error}"));
}

#[test]
fn a_use_from_a_nullable_pointer_into_a_box_is_refused() {
    let body = body_assigning(
        optional(node()),
        optional(TypeKind::Int),
        Rvalue::Use(copy_of_source()),
    );
    assert_refused(&body, "laid out");
}

#[test]
fn a_cast_from_a_box_into_a_nullable_pointer_is_refused() {
    let body = body_assigning(
        optional(TypeKind::Int),
        optional(node()),
        Rvalue::Cast(Box::new(copy_of_source()), ty(optional(node()))),
    );
    assert_refused(&body, "laid out");
}

#[test]
fn a_some_built_boxed_into_a_nullable_pointer_slot_is_refused() {
    let payload = Operand::Constant(Box::new(Constant {
        span: Span::default(),
        ty: ty(TypeKind::Int),
        literal: Literal::Integer(IntegerLiteral::I64(1)),
    }));
    let body = body_assigning(
        TypeKind::Int,
        optional(node()),
        Rvalue::Aggregate(
            AggregateKind::Option(ty(optional(TypeKind::Int))),
            vec![payload],
        ),
    );
    assert_refused(&body, "laid out");
}

#[test]
fn an_optional_at_the_error_type_is_refused() {
    let body = body_assigning(
        optional(TypeKind::Error),
        optional(node()),
        Rvalue::Use(copy_of_source()),
    );
    assert_refused(&body, "error type");
}

#[test]
fn an_optional_of_a_parameter_the_body_leaves_closed_is_refused() {
    let body = body_assigning(
        optional(parameter()),
        optional(node()),
        Rvalue::Use(copy_of_source()),
    );
    assert_refused(&body, "type parameter `T`");
}
