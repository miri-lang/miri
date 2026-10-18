// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rule that two optionals meet only where they share a layout.
//!
//! An optional of a class is laid out as the nullable class pointer itself,
//! while an optional of a scalar or of an unresolved type parameter is a box.
//! Each fixture is a hand-built MIR body pinning one place such optionals meet,
//! once with layouts that disagree and once with layouts that agree.

use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::place::PlaceElem;
use miri::mir::symbol::Symbol;
use miri::mir::verify::{
    verify_optional_call_layouts, verify_optional_representation, VerificationViolation,
};
use miri::mir::{
    AggregateKind, Body, Constant, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
};
use miri::type_checker::context::{ClassDefinition, TypeDefinition};
use miri::type_checker::ModuleId;
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn span() -> Span {
    Span::new(0, 0)
}

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

fn optional(kind: TypeKind) -> TypeKind {
    TypeKind::Option(Box::new(ty(kind)))
}

fn node() -> TypeKind {
    TypeKind::Custom("Node".to_string(), None)
}

/// A type parameter no substitution reached.
fn parameter() -> TypeKind {
    TypeKind::Custom("T".to_string(), None)
}

fn definitions() -> HashMap<String, TypeDefinition> {
    let node = TypeDefinition::Class(ClassDefinition {
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
    });
    HashMap::from([("Node".to_string(), node)])
}

fn place(local: usize) -> Place {
    Place::new(Local(local))
}

fn payload(local: usize) -> Place {
    Place {
        local: Local(local),
        projection: vec![PlaceElem::Field(0)],
    }
}

fn stmt(kind: StatementKind) -> Statement {
    Statement { kind, span: span() }
}

fn assign(dest: Place, rvalue: Rvalue) -> Statement {
    stmt(StatementKind::Assign(dest, rvalue))
}

fn copy(local: usize) -> Operand {
    Operand::Copy(place(local))
}

fn none_of(kind: TypeKind) -> Operand {
    Operand::Constant(Box::new(Constant {
        ty: ty(kind),
        literal: Literal::None,
        span: span(),
    }))
}

fn int_const() -> Operand {
    Operand::Constant(Box::new(Constant {
        ty: ty(TypeKind::Int),
        literal: Literal::Integer(IntegerLiteral::I32(1)),
        span: span(),
    }))
}

fn ret_block(statements: Vec<Statement>) -> BasicBlockData {
    BasicBlockData {
        statements,
        terminator: Some(Terminator {
            kind: TerminatorKind::Return,
            span: span(),
        }),
        is_cleanup: false,
    }
}

/// A body whose local `i` has kind `kinds[i]`; local 0 is the return slot and
/// locals `1..=arg_count` are the parameters.
fn body_of(kinds: &[TypeKind], arg_count: usize, blocks: Vec<BasicBlockData>) -> Body {
    let mut body = Body::new(arg_count, span(), ExecutionModel::Cpu);
    for kind in kinds {
        body.new_local(LocalDecl::new(ty(kind.clone()), span()));
    }
    body.basic_blocks.extend(blocks);
    body
}

fn findings(body: &Body) -> Vec<VerificationViolation> {
    verify_optional_representation(body, &definitions())
}

fn messages(violations: &[VerificationViolation]) -> String {
    violations
        .iter()
        .map(VerificationViolation::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

fn assert_flagged(body: &Body, fragments: &[&str]) {
    let violations = findings(body);
    let text = messages(&violations);
    assert_eq!(violations.len(), 1, "expected one finding, got: {text}");
    for fragment in fragments {
        assert!(text.contains(fragment), "`{fragment}` missing from: {text}");
    }
}

fn assert_clean(body: &Body) {
    let violations = findings(body);
    assert!(
        violations.is_empty(),
        "expected no findings, got: {}",
        messages(&violations)
    );
}

/// Locals: 0 the return slot, 1 an unresolved `T?`, 2 a `Node?`, 3 a `Node`.
fn meeting_kinds() -> Vec<TypeKind> {
    vec![
        TypeKind::Void,
        optional(parameter()),
        optional(node()),
        node(),
    ]
}

#[test]
fn unresolved_optional_used_as_a_pointer_optional_is_flagged() {
    let body = body_of(
        &meeting_kinds(),
        0,
        vec![ret_block(vec![assign(place(2), Rvalue::Use(copy(1)))])],
    );
    assert_flagged(&body, &["_2", "_1", "T?", "Node?"]);
}

#[test]
fn cast_between_optionals_of_different_layouts_is_flagged() {
    let cast = Rvalue::Cast(Box::new(copy(1)), ty(optional(node())));
    let body = body_of(
        &meeting_kinds(),
        0,
        vec![ret_block(vec![assign(place(2), cast)])],
    );
    assert_flagged(&body, &["_2", "T?", "Node?"]);
}

#[test]
fn aggregate_slot_of_a_different_optional_layout_is_flagged() {
    let mut kinds = meeting_kinds();
    kinds.push(optional(optional(node())));
    let wrap = Rvalue::Aggregate(
        AggregateKind::Option(ty(optional(optional(node())))),
        vec![copy(1)],
    );
    let body = body_of(&kinds, 0, vec![ret_block(vec![assign(place(4), wrap)])]);
    assert_flagged(&body, &["_4", "T?", "Node?"]);
}

#[test]
fn some_built_at_a_different_optional_layout_than_its_destination_is_flagged() {
    // `Some(node)` says it builds a `Node??` but is written into a `Node?`.
    let wrap = Rvalue::Aggregate(
        AggregateKind::Option(ty(optional(optional(node())))),
        vec![copy(3)],
    );
    let body = body_of(
        &meeting_kinds(),
        0,
        vec![ret_block(vec![assign(place(2), wrap)])],
    );
    assert_flagged(&body, &["a `Some` built as a Node??", "into `_2` at Node?"]);
}

#[test]
fn some_built_at_its_destination_type_meets_cleanly() {
    let mut kinds = meeting_kinds();
    kinds.push(optional(optional(node())));
    let wrap = Rvalue::Aggregate(
        AggregateKind::Option(ty(optional(optional(node())))),
        vec![copy(2)],
    );
    let body = body_of(&kinds, 0, vec![ret_block(vec![assign(place(4), wrap)])]);
    assert_clean(&body);
}

#[test]
fn store_into_the_payload_of_a_pointer_optional_is_flagged() {
    let body = body_of(
        &meeting_kinds(),
        0,
        vec![ret_block(vec![assign(payload(2), Rvalue::Use(copy(3)))])],
    );
    assert_flagged(&body, &["_2", "Node?", "field 0"]);
}

#[test]
fn reference_to_the_payload_of_a_pointer_optional_is_flagged() {
    let mut kinds = meeting_kinds();
    kinds.push(TypeKind::RawPtr);
    let body = body_of(
        &kinds,
        0,
        vec![ret_block(vec![assign(place(4), Rvalue::Ref(payload(2)))])],
    );
    assert_flagged(&body, &["_2", "Node?", "field 0"]);
}

#[test]
fn dealloc_of_the_payload_of_a_pointer_optional_is_flagged() {
    let body = body_of(
        &meeting_kinds(),
        0,
        vec![ret_block(vec![stmt(StatementKind::Dealloc(payload(2)))])],
    );
    assert_flagged(&body, &["_2", "Node?", "field 0"]);
}

#[test]
fn call_writing_the_payload_of_a_pointer_optional_is_flagged() {
    let call = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::runtime("make_node", span()),
            args: Vec::new(),
            arg_modes: Vec::new(),
            arg_handles: Vec::new(),
            destination: payload(2),
            target: Some(BasicBlock(1)),
        },
        span: span(),
    };
    let first = BasicBlockData {
        statements: Vec::new(),
        terminator: Some(call),
        is_cleanup: false,
    };
    let body = body_of(&meeting_kinds(), 0, vec![first, ret_block(Vec::new())]);
    assert_flagged(&body, &["_2", "Node?", "field 0"]);
}

#[test]
fn optionals_sharing_a_layout_meet_cleanly() {
    // An optional of an untracked word is boxed, like one of an unresolved
    // payload; an optional of a scalar is held as its bytes instead.
    let kinds = vec![
        TypeKind::Void,
        optional(parameter()),
        optional(TypeKind::RawPtr),
        optional(node()),
        optional(optional(node())),
        node(),
    ];
    let statements = vec![
        // Two boxes: an unresolved payload and an untracked word.
        assign(place(2), Rvalue::Use(copy(1))),
        // A boxed optional's payload is a slot of its box.
        assign(payload(2), Rvalue::Use(int_const())),
        // The absent value is null under every layout.
        assign(place(3), Rvalue::Use(none_of(optional(parameter())))),
        // The outer layer of a nested optional stays a box.
        assign(payload(4), Rvalue::Use(copy(3))),
        // Reading a pointer optional's payload is the pointer itself.
        assign(place(5), Rvalue::Use(Operand::Copy(payload(3)))),
        assign(
            place(4),
            Rvalue::Aggregate(
                AggregateKind::Option(ty(optional(optional(node())))),
                vec![copy(3)],
            ),
        ),
    ];
    let body = body_of(&kinds, 0, vec![ret_block(statements)]);
    assert_clean(&body);
}

fn callee_symbol() -> Symbol {
    Symbol::declared_function(&ModuleId::Program, "callee")
}

/// A function taking a `parameter_kind` and returning a `return_kind`.
fn callee_body(parameter_kind: TypeKind, return_kind: TypeKind) -> Body {
    body_of(
        &[return_kind, parameter_kind],
        1,
        vec![ret_block(Vec::new())],
    )
}

/// A caller passing its local 1 to `callee` and receiving into its local 2.
fn caller_body(argument_kind: TypeKind, destination_kind: TypeKind) -> Body {
    let mut function = Operand::function(callee_symbol(), span());
    if let Operand::Function(constant) = &mut function {
        constant.ty = ty(destination_kind.clone());
    }
    let call = Terminator {
        kind: TerminatorKind::Call {
            func: function,
            args: vec![copy(1)],
            arg_modes: Vec::new(),
            arg_handles: Vec::new(),
            destination: place(2),
            target: Some(BasicBlock(1)),
        },
        span: span(),
    };
    let first = BasicBlockData {
        statements: Vec::new(),
        terminator: Some(call),
        is_cleanup: false,
    };
    body_of(
        &[TypeKind::Void, argument_kind, destination_kind],
        0,
        vec![first, ret_block(Vec::new())],
    )
}

fn call_findings(caller: Body, callee: Body) -> Vec<(Symbol, VerificationViolation)> {
    let caller_symbol = Symbol::declared_function(&ModuleId::Program, "caller");
    let program = vec![(caller_symbol, caller), (callee_symbol(), callee)];
    verify_optional_call_layouts(&program, &definitions())
}

fn call_messages(violations: &[(Symbol, VerificationViolation)]) -> String {
    violations
        .iter()
        .map(|(symbol, violation)| format!("{}: {violation}", symbol.link_name()))
        .collect::<Vec<_>>()
        .join("; ")
}

#[test]
fn argument_of_a_different_optional_layout_than_its_parameter_is_flagged() {
    let violations = call_findings(
        caller_body(optional(node()), TypeKind::Void),
        callee_body(optional(parameter()), TypeKind::Void),
    );
    let text = call_messages(&violations);
    assert_eq!(violations.len(), 1, "expected one finding, got: {text}");
    assert!(text.contains("argument 0"), "{text}");
    assert!(text.contains("Node?") && text.contains("T?"), "{text}");
}

#[test]
fn destination_of_a_different_optional_layout_than_the_return_is_flagged() {
    let violations = call_findings(
        caller_body(TypeKind::Int, optional(node())),
        callee_body(TypeKind::Int, optional(parameter())),
    );
    let text = call_messages(&violations);
    assert_eq!(violations.len(), 1, "expected one finding, got: {text}");
    assert!(text.contains("_2") && text.contains("returns"), "{text}");
    assert!(text.contains("Node?") && text.contains("T?"), "{text}");
}

#[test]
fn call_whose_optionals_share_the_callee_layouts_is_clean() {
    let violations = call_findings(
        caller_body(optional(node()), optional(TypeKind::RawPtr)),
        callee_body(optional(node()), optional(parameter())),
    );
    assert!(violations.is_empty(), "{}", call_messages(&violations));
}

/// A finding describes each side's layout layer by layer, outermost first.
#[test]
fn a_layout_mismatch_describes_the_layers_of_each_side() {
    let unresolved = body_of(
        &meeting_kinds(),
        0,
        vec![ret_block(vec![assign(place(2), Rvalue::Use(copy(1)))])],
    );
    assert_flagged(
        &unresolved,
        &[
            "laid out as one box,",
            "laid out as the nullable payload pointer",
        ],
    );
    let wrap = Rvalue::Aggregate(
        AggregateKind::Option(ty(optional(optional(node())))),
        vec![copy(3)],
    );
    let nested = body_of(
        &meeting_kinds(),
        0,
        vec![ret_block(vec![assign(place(2), wrap)])],
    );
    assert_flagged(
        &nested,
        &["laid out as one box around the nullable payload pointer"],
    );
}

/// An optional held as its bytes meets one held in a box as no one value.
#[test]
fn an_inline_optional_meeting_a_boxed_one_is_flagged() {
    let kinds = vec![
        TypeKind::Void,
        optional(parameter()),
        optional(TypeKind::Int),
    ];
    let body = body_of(
        &kinds,
        0,
        vec![ret_block(vec![assign(place(2), Rvalue::Use(copy(1)))])],
    );
    assert_flagged(
        &body,
        &["laid out as one box", "laid out as one inline layer"],
    );
}
