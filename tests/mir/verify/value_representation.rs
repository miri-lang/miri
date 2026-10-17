// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rule that a bitwise value held as its bytes is never counted,
//! never has its address escape into a heap slot, and never meets a counted
//! word.
//!
//! Every program classifies a bitwise value as counted today, so the rule has
//! nothing to report on one. Each fixture is a hand-built body whose classifier
//! treats bitwise values as uncounted, the representation the rule guards, and
//! pins one violation; the same body under the ordinary classifier is clean.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::place::PlaceElem;
use miri::mir::rc::RcClassifier;
use miri::mir::symbol::Symbol;
use miri::mir::verify::{
    verify_value_call_classes, verify_value_representation, VerificationViolation,
};
use miri::mir::{
    AggregateKind, Body, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use miri::type_checker::context::TypeDefinition;
use miri::type_checker::ModuleId;
use std::collections::HashMap;
use std::rc::Rc;

fn span() -> Span {
    Span::new(0, 0)
}

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

fn type_expr(kind: TypeKind) -> Expression {
    Expression {
        id: 0,
        node: ExpressionKind::Type(Box::new(ty(kind)), false),
        span: span(),
    }
}

fn tuple(kinds: Vec<TypeKind>) -> TypeKind {
    TypeKind::Tuple(kinds.into_iter().map(type_expr).collect())
}

/// A bitwise value: two scalars.
fn pair() -> TypeKind {
    tuple(vec![TypeKind::Int, TypeKind::Int])
}

/// A counted tuple whose first element is a word slot.
fn holder() -> TypeKind {
    tuple(vec![TypeKind::RawPtr, TypeKind::String])
}

fn definitions() -> HashMap<String, TypeDefinition> {
    HashMap::new()
}

fn place(local: usize) -> Place {
    Place::new(Local(local))
}

fn field(local: usize, index: usize) -> Place {
    Place {
        local: Local(local),
        projection: vec![PlaceElem::Field(index)],
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

fn block(statements: Vec<Statement>, kind: TerminatorKind) -> BasicBlockData {
    BasicBlockData {
        statements,
        terminator: Some(Terminator { kind, span: span() }),
        is_cleanup: false,
    }
}

fn ret_block(statements: Vec<Statement>) -> BasicBlockData {
    block(statements, TerminatorKind::Return)
}

/// Locals: 0 the return slot, 1 a `(int, int)` value, 2 a raw pointer, 3 a
/// `String`, 4 a counted `(RawPtr, String)`, 5 a closure.
fn kinds() -> Vec<TypeKind> {
    vec![
        TypeKind::Void,
        pair(),
        TypeKind::RawPtr,
        TypeKind::String,
        holder(),
        TypeKind::RawPtr,
    ]
}

/// A body whose local `i` has kind `kinds[i]`, classified with bitwise
/// values held as their bytes when `is_uncounted`.
fn body_of(
    kinds: &[TypeKind],
    arg_count: usize,
    blocks: Vec<BasicBlockData>,
    is_uncounted: bool,
) -> Body {
    let mut body = Body::new(arg_count, span(), ExecutionModel::Cpu);
    for kind in kinds {
        body.new_local(LocalDecl::new(ty(kind.clone()), span()));
    }
    body.basic_blocks.extend(blocks);
    let table = Rc::new(definitions());
    body.rc_classifier = if is_uncounted {
        RcClassifier::with_uncounted_bitwise_values(table)
    } else {
        RcClassifier::new(table)
    };
    body
}

fn value_body(statements: Vec<Statement>) -> Body {
    body_of(&kinds(), 0, vec![ret_block(statements)], true)
}

fn messages(violations: &[VerificationViolation]) -> String {
    violations
        .iter()
        .map(VerificationViolation::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Exactly one finding carrying every fragment, and none at all once the
/// same statements are classified as every program is today.
fn assert_flagged(statements: Vec<Statement>, fragments: &[&str]) {
    let violations = verify_value_representation(&value_body(statements.clone()), &definitions());
    let text = messages(&violations);
    assert_eq!(violations.len(), 1, "expected one finding, got: {text}");
    for fragment in fragments {
        assert!(text.contains(fragment), "`{fragment}` missing from: {text}");
    }
    let counted = body_of(&kinds(), 0, vec![ret_block(statements)], false);
    assert_clean(&counted);
}

fn assert_clean(body: &Body) {
    let violations = verify_value_representation(body, &definitions());
    assert!(
        violations.is_empty(),
        "expected no findings, got: {}",
        messages(&violations)
    );
}

fn address_of_pair() -> Statement {
    assign(place(2), Rvalue::Ref(place(1)))
}

#[test]
fn increment_of_a_value_is_flagged() {
    assert_flagged(
        vec![stmt(StatementKind::IncRef(place(1)))],
        &["_1", "IncRef", "(int, int)", "bitwise value"],
    );
}

#[test]
fn decrement_of_a_value_is_flagged() {
    assert_flagged(
        vec![stmt(StatementKind::DecRef(place(1)))],
        &["_1", "DecRef", "(int, int)"],
    );
}

#[test]
fn deallocation_of_a_value_is_flagged() {
    assert_flagged(
        vec![stmt(StatementKind::Dealloc(place(1)))],
        &["_1", "Dealloc", "(int, int)"],
    );
}

#[test]
fn address_of_a_value_stored_into_a_counted_field_is_flagged() {
    assert_flagged(
        vec![address_of_pair(), assign(field(4, 0), Rvalue::Use(copy(2)))],
        &["`_4.0`", "address of `_1`", "stored"],
    );
}

#[test]
fn address_of_a_value_written_straight_into_a_field_is_flagged() {
    assert_flagged(
        vec![assign(field(4, 0), Rvalue::Ref(place(1)))],
        &["`_4.0`", "address of `_1`"],
    );
}

#[test]
fn address_of_a_value_captured_by_a_closure_is_flagged() {
    let capture = Rvalue::Aggregate(
        AggregateKind::Closure(
            Symbol::declared_function(&ModuleId::Program, "lambda"),
            ty(TypeKind::RawPtr),
        ),
        vec![copy(2)],
    );
    assert_flagged(
        vec![address_of_pair(), assign(place(5), capture)],
        &["`_5`", "address of `_1`", "closure"],
    );
}

#[test]
fn address_of_a_value_held_in_an_enum_payload_is_flagged() {
    let payload = Rvalue::Aggregate(
        AggregateKind::Enum(Rc::from("Slot"), Rc::from("Held")),
        vec![copy(2)],
    );
    assert_flagged(
        vec![address_of_pair(), assign(place(5), payload)],
        &["`_5`", "address of `_1`", "enum"],
    );
}

#[test]
fn address_of_a_value_carried_through_a_copy_and_returned_is_flagged() {
    let mut locals = kinds();
    locals[0] = TypeKind::RawPtr;
    let statements = vec![
        address_of_pair(),
        assign(place(5), Rvalue::Use(copy(2))),
        assign(place(0), Rvalue::Use(copy(5))),
    ];
    let body = body_of(&locals, 0, vec![ret_block(statements)], true);
    let violations = verify_value_representation(&body, &definitions());
    let text = messages(&violations);
    assert_eq!(violations.len(), 1, "expected one finding, got: {text}");
    assert!(
        text.contains("address of `_1`") && text.contains("returned"),
        "{text}"
    );
}

#[test]
fn value_written_into_a_counted_word_is_flagged() {
    assert_flagged(
        vec![assign(place(3), Rvalue::Use(copy(1)))],
        &[
            "`_3`",
            "(int, int)",
            "bitwise value",
            "String",
            "counted word",
        ],
    );
}

#[test]
fn counted_word_written_into_a_value_is_flagged() {
    assert_flagged(
        vec![assign(place(1), Rvalue::Use(copy(3)))],
        &["`_1`", "String", "counted word", "bitwise value"],
    );
}

#[test]
fn cast_of_a_value_to_a_counted_word_is_flagged() {
    let cast = Rvalue::Cast(Box::new(copy(1)), ty(TypeKind::String));
    assert_flagged(
        vec![assign(place(3), cast)],
        &["cast", "(int, int)", "String"],
    );
}

#[test]
fn value_in_a_counted_aggregate_slot_is_flagged() {
    let built = Rvalue::Aggregate(AggregateKind::Tuple, vec![copy(2), copy(1)]);
    assert_flagged(
        vec![assign(place(4), built)],
        &["aggregate slot", "`_4`", "(int, int)", "String"],
    );
}

#[test]
fn value_used_by_value_and_by_address_in_a_call_is_clean() {
    let mut locals = kinds();
    locals.push(pair());
    let call = TerminatorKind::Call {
        func: Operand::runtime("consume_pointer", span()),
        args: vec![copy(2)],
        arg_modes: Vec::new(),
        arg_handles: Vec::new(),
        destination: place(0),
        target: Some(BasicBlock(1)),
    };
    let statements = vec![
        assign(place(6), Rvalue::Use(copy(1))),
        address_of_pair(),
        stmt(StatementKind::IncRef(place(3))),
        stmt(StatementKind::DecRef(place(3))),
        assign(field(4, 1), Rvalue::Use(copy(3))),
    ];
    let body = body_of(
        &locals,
        0,
        vec![block(statements, call), ret_block(Vec::new())],
        true,
    );
    assert_clean(&body);
}

fn callee_symbol() -> Symbol {
    Symbol::declared_function(&ModuleId::Program, "callee")
}

/// A caller passing its local 1 to `callee` and receiving into its local 2.
fn caller_body(argument: TypeKind, destination: TypeKind) -> Body {
    let mut function = Operand::function(callee_symbol(), span());
    if let Operand::Function(constant) = &mut function {
        constant.ty = ty(destination.clone());
    }
    let call = TerminatorKind::Call {
        func: function,
        args: vec![copy(1)],
        arg_modes: Vec::new(),
        arg_handles: Vec::new(),
        destination: place(2),
        target: Some(BasicBlock(1)),
    };
    body_of(
        &[TypeKind::Void, argument, destination],
        0,
        vec![block(Vec::new(), call), ret_block(Vec::new())],
        true,
    )
}

/// A function taking a `parameter` and returning a `returned`.
fn callee_body(parameter: TypeKind, returned: TypeKind) -> Body {
    body_of(&[returned, parameter], 1, vec![ret_block(Vec::new())], true)
}

fn call_findings(caller: Body, callee: Body) -> String {
    let program = vec![
        (
            Symbol::declared_function(&ModuleId::Program, "caller"),
            caller,
        ),
        (callee_symbol(), callee),
    ];
    verify_value_call_classes(&program, &definitions())
        .iter()
        .map(|(symbol, violation)| format!("{}: {violation}", symbol.link_name()))
        .collect::<Vec<_>>()
        .join("; ")
}

#[test]
fn counted_argument_bound_to_a_value_parameter_is_flagged() {
    let text = call_findings(
        caller_body(TypeKind::String, TypeKind::Void),
        callee_body(pair(), TypeKind::Void),
    );
    assert!(
        text.contains("argument 0") && text.contains("String"),
        "{text}"
    );
    assert!(text.contains("(int, int)") && !text.contains(';'), "{text}");
}

#[test]
fn value_result_received_into_a_counted_word_is_flagged() {
    let text = call_findings(
        caller_body(pair(), TypeKind::String),
        callee_body(pair(), pair()),
    );
    assert!(text.contains("returns") && text.contains("`_2`"), "{text}");
    assert!(text.contains("String") && !text.contains(';'), "{text}");
}

#[test]
fn call_matching_its_callee_classes_is_clean() {
    let text = call_findings(caller_body(pair(), pair()), callee_body(pair(), pair()));
    assert!(text.is_empty(), "expected no findings, got: {text}");
}

#[test]
fn value_call_mismatch_is_silent_while_bitwise_values_are_counted() {
    let mut caller = caller_body(TypeKind::String, TypeKind::Void);
    let mut callee = callee_body(pair(), TypeKind::Void);
    caller.rc_classifier = RcClassifier::new(Rc::new(definitions()));
    callee.rc_classifier = RcClassifier::new(Rc::new(definitions()));
    let text = call_findings(caller, callee);
    assert!(text.is_empty(), "expected no findings, got: {text}");
}
