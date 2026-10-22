// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rule that a bitwise value held as its bytes is never counted,
//! never has its address escape into a heap slot, and never meets a counted
//! word.
//!
//! Each fixture is a hand-built body over a tuple of scalars, which every
//! program holds as its bytes, and pins one violation.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{FunctionTypeData, Type, TypeDeclarationKind, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::place::PlaceElem;
use miri::mir::rc::RcClassifier;
use miri::mir::symbol::Symbol;
use miri::mir::verify::{
    verify_bound_names, verify_optional_slots, verify_value_call_classes,
    verify_value_representation, VerificationViolation,
};
use miri::mir::{
    AggregateKind, Body, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use miri::type_checker::context::{EnumDefinition, GenericDefinition, TypeDefinition};
use miri::type_checker::ModuleId;
use std::collections::{BTreeMap, HashMap};
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

/// A body whose local `i` has kind `kinds[i]`.
fn body_of(kinds: &[TypeKind], arg_count: usize, blocks: Vec<BasicBlockData>) -> Body {
    let mut body = Body::new(arg_count, span(), ExecutionModel::Cpu);
    for kind in kinds {
        body.new_local(LocalDecl::new(ty(kind.clone()), span()));
    }
    body.basic_blocks.extend(blocks);
    body.rc_classifier = RcClassifier::new(Rc::new(definitions()));
    body
}

fn value_body(statements: Vec<Statement>) -> Body {
    body_of(&kinds(), 0, vec![ret_block(statements)])
}

fn messages(violations: &[VerificationViolation]) -> String {
    violations
        .iter()
        .map(VerificationViolation::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Exactly one finding carrying every fragment.
fn assert_flagged(statements: Vec<Statement>, fragments: &[&str]) {
    let violations = verify_value_representation(&value_body(statements), &definitions());
    let text = messages(&violations);
    assert_eq!(violations.len(), 1, "expected one finding, got: {text}");
    for fragment in fragments {
        assert!(text.contains(fragment), "`{fragment}` missing from: {text}");
    }
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
    let body = body_of(&locals, 0, vec![ret_block(statements)]);
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
    let built = Rvalue::Aggregate(AggregateKind::Tuple(ty(holder())), vec![copy(2), copy(1)]);
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
    )
}

/// A function taking a `parameter` and returning a `returned`.
fn callee_body(parameter: TypeKind, returned: TypeKind) -> Body {
    body_of(&[returned, parameter], 1, vec![ret_block(Vec::new())])
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
fn value_call_mismatch_is_flagged_as_every_program_is_classified() {
    let mut caller = caller_body(TypeKind::String, TypeKind::Void);
    let mut callee = callee_body(pair(), TypeKind::Void);
    caller.rc_classifier = RcClassifier::new(Rc::new(definitions()));
    callee.rc_classifier = RcClassifier::new(Rc::new(definitions()));
    let text = call_findings(caller, callee);
    assert!(
        text.contains("argument 0") && text.contains("held as its bytes"),
        "{text}"
    );
}

/// A body whose classifier was never set reads the type table it is handed
/// no differently: a tuple of scalars and an `int?` are still held as their
/// bytes, and meeting a counted word still raises a finding.
#[test]
fn a_body_with_the_default_classifier_raises_the_same_finding() {
    let optional = TypeKind::Option(Box::new(ty(TypeKind::Int)));
    let mut body = Body::new(0, span(), ExecutionModel::Cpu);
    for kind in [
        TypeKind::Void,
        TypeKind::Option(Box::new(ty(TypeKind::String))),
        optional,
    ] {
        body.new_local(LocalDecl::new(ty(kind), span()));
    }
    body.basic_blocks
        .push(ret_block(vec![assign(place(2), Rvalue::Use(copy(1)))]));
    let text = messages(&verify_value_representation(&body, &definitions()));
    assert!(text.contains("bytes and a word meet"), "{text}");
}

/// The absent value handed as a constant where a callee takes an optional
/// held as its bytes is an unsettled seam: a constant has no bytes to pass.
#[test]
fn an_absent_constant_passed_to_an_optional_held_as_bytes_is_flagged() {
    let optional = TypeKind::Option(Box::new(ty(TypeKind::Int)));
    let callee_symbol = Symbol::declared_function(&ModuleId::Program, "takes");
    let callee = body_of(
        &[TypeKind::Void, optional.clone()],
        1,
        vec![ret_block(Vec::new())],
    );
    let absent = Operand::Constant(Box::new(miri::mir::Constant {
        span: span(),
        ty: ty(optional),
        literal: miri::ast::literal::Literal::None,
    }));
    let call = TerminatorKind::Call {
        func: Operand::function(callee_symbol.clone(), span()),
        args: vec![absent],
        arg_modes: Vec::new(),
        arg_handles: Vec::new(),
        destination: place(0),
        target: Some(BasicBlock(1)),
    };
    let caller = body_of(
        &[TypeKind::Void],
        0,
        vec![block(Vec::new(), call), ret_block(Vec::new())],
    );
    let caller_symbol = Symbol::declared_function(&ModuleId::Program, "caller");
    let program = vec![(caller_symbol, caller), (callee_symbol, callee)];
    let found = verify_optional_slots(&program, Symbol::link_name);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].1.message.contains("without those bytes"),
        "{found:?}"
    );
}

/// The absent constant handed to a parameter whose type the call site could
/// not record is refused: nothing says the bytes it needs are there.
#[test]
fn an_absent_constant_at_an_unreadable_parameter_is_flagged() {
    let absent = Operand::Constant(Box::new(miri::mir::Constant {
        span: span(),
        ty: ty(TypeKind::Option(Box::new(ty(TypeKind::Void)))),
        literal: miri::ast::literal::Literal::None,
    }));
    let call = TerminatorKind::VirtualCall {
        vtable_slot: 1,
        args: vec![copy(1), absent],
        arg_modes: Vec::new(),
        param_types: vec![None],
        destination: place(0),
        target: Some(BasicBlock(1)),
    };
    let caller = body_of(
        &[TypeKind::Void, TypeKind::String],
        0,
        vec![block(Vec::new(), call), ret_block(Vec::new())],
    );
    let symbol = Symbol::declared_function(&ModuleId::Program, "caller");
    let found = verify_optional_slots(&[(symbol, caller)], Symbol::link_name);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].1.message.contains("cannot be read"), "{found:?}");
}

/// A body compiled for one instantiation declares no local at a parameter
/// it neither leaves open nor the program defines: such a `U?` missed its
/// substitution, and has no layout to agree on.
#[test]
fn a_local_naming_an_unbound_parameter_is_flagged() {
    let unbound = TypeKind::Option(Box::new(ty(TypeKind::Custom("U".to_string(), None))));
    let body = body_of(&[TypeKind::Void, unbound], 0, vec![ret_block(Vec::new())]);
    let text = messages(&verify_bound_names(&body));
    assert!(text.contains("`_1` is declared at `U?`"), "{text}");

    let mut open = body_of(
        &[
            TypeKind::Void,
            TypeKind::Option(Box::new(ty(TypeKind::Custom("U".to_string(), None)))),
        ],
        0,
        vec![ret_block(Vec::new())],
    );
    open.open_params.insert("U".to_string());
    assert!(
        verify_bound_names(&open).is_empty(),
        "an open parameter is bound"
    );
}

/// `enum Outcome<A, B>`, its two variants carrying one parameter each.
fn outcome_definitions() -> HashMap<String, TypeDefinition> {
    let generic = |name: &str| GenericDefinition {
        name: name.to_string(),
        constraint: None,
        kind: TypeDeclarationKind::None,
    };
    let outcome = TypeDefinition::Enum(EnumDefinition {
        variants: BTreeMap::from([
            ("Good".to_string(), vec![ty(custom("A"))]),
            ("Bad".to_string(), vec![ty(custom("B"))]),
        ]),
        generics: Some(vec![generic("A"), generic("B")]),
        methods: BTreeMap::new(),
        module: String::new(),
        must_use: false,
        non_exhaustive: false,
    });
    HashMap::from([("Outcome".to_string(), outcome)])
}

fn custom(name: &str) -> TypeKind {
    TypeKind::Custom(name.to_string(), None)
}

#[test]
fn an_unsettled_enum_argument_is_bound_only_at_its_position_and_in_its_arm_binding() {
    let outcome = |a: TypeKind, b: TypeKind| {
        TypeKind::Custom(
            "Outcome".to_string(),
            Some(vec![type_expr(a), type_expr(b)]),
        )
    };
    let arm = assign(place(2), Rvalue::Use(Operand::Copy(field(1, 1))));
    let mut body = body_of(
        &[
            TypeKind::Void,
            outcome(TypeKind::Float, custom("B")),
            custom("B"),
            custom("A"),
            custom("B"),
            outcome(custom("B"), TypeKind::Float),
        ],
        0,
        vec![ret_block(vec![arm])],
    );
    body.rc_classifier = RcClassifier::new(Rc::new(outcome_definitions()));
    let text = messages(&verify_bound_names(&body));
    assert!(
        !text.contains("`_1`"),
        "`B` in its own position is bound: {text}"
    );
    assert!(
        !text.contains("`_2`"),
        "the arm binding at `B` is bound: {text}"
    );
    assert!(text.contains("`_3` is declared at `A`"), "{text}");
    assert!(
        text.contains("`_4` is declared at `B`"),
        "no arm binds `_4`: {text}"
    );
    assert!(
        text.contains("`_5` is declared at `Outcome<B, float>`"),
        "`B` in `A`'s position is a miss: {text}"
    );
}

#[test]
fn a_function_signature_is_checked_with_a_private_type_written_by_its_name() {
    let returning = |kind: TypeKind| {
        TypeKind::Function(Box::new(FunctionTypeData {
            generics: None,
            params: Vec::new(),
            return_type: Some(Box::new(type_expr(kind))),
        }))
    };
    let mut body = body_of(
        &[
            TypeKind::Void,
            returning(custom("U")),
            returning(custom("Hidden")),
            TypeKind::Option(Box::new(ty(custom("Hidden")))),
        ],
        0,
        vec![ret_block(Vec::new())],
    );
    let mut definitions = outcome_definitions();
    if let Some(private) = definitions.get("Outcome").cloned() {
        definitions.insert("local.k.a.Hidden".to_string(), private);
    }
    body.rc_classifier = RcClassifier::new(Rc::new(definitions));
    let text = messages(&verify_bound_names(&body));
    assert!(
        text.contains("`_1` is declared at `Function() -> U`"),
        "a signature is looked into: {text}"
    );
    assert!(
        !text.contains("`_2`"),
        "a private type by its name in a signature: {text}"
    );
    assert!(text.contains("`_3` is declared at `Hidden?`"), "{text}");
}
