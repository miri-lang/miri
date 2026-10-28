// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the whole-program analysis of which calls may release a value
//! their caller lends them.
//!
//! Each program is a handful of hand-built bodies over two classes, `Node`
//! holding an int and `Holder` holding a `Node`, so each test pins the one
//! operation that makes a body loud or leaves it quiet.

use miri::ast::common::MemberVisibility;
use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::analysis::heap_effects::{stamp_quiet_calls, HeapEffects, Phase};
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::place::PlaceElem;
use miri::mir::rc::RcClassifier;
use miri::mir::symbol::Symbol;
use miri::mir::type_facts::DropHookReach;
use miri::mir::{
    Body, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue, Statement, StatementKind,
    Terminator, TerminatorKind,
};
use miri::runtime_fns::rt;
use miri::type_checker::context::{ClassDefinition, FieldInfo, MethodInfo, TypeDefinition};
use miri::type_checker::ModuleId;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

pub fn span() -> Span {
    Span::new(0, 0)
}

pub fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

pub fn node() -> TypeKind {
    TypeKind::Custom("Node".to_string(), None)
}

pub fn holder() -> TypeKind {
    TypeKind::Custom("Holder".to_string(), None)
}

pub fn int_list() -> TypeKind {
    TypeKind::List(Box::new(type_expression(TypeKind::Int)))
}

fn class(name: &str, field: &str, field_kind: TypeKind) -> TypeDefinition {
    let info = FieldInfo {
        ty: ty(field_kind),
        mutable: true,
        visibility: MemberVisibility::Public,
        initializer: None,
    };
    TypeDefinition::Class(ClassDefinition {
        name: name.to_string(),
        generics: None,
        base_class: None,
        base_class_args: None,
        traits: Vec::new(),
        trait_args: HashMap::new(),
        fields: vec![(field.to_string(), info)],
        methods: BTreeMap::new(),
        module: "test".to_string(),
        is_abstract: false,
        runtime_settled_methods: BTreeSet::new(),
    })
}

/// `Shelf`, a class holding a list of ints.
pub fn shelf() -> TypeKind {
    TypeKind::Custom("Shelf".to_string(), None)
}

/// `Registry`, a class holding a map from ints to nodes.
pub fn registry() -> TypeKind {
    TypeKind::Custom("Registry".to_string(), None)
}

fn type_expression(kind: TypeKind) -> Expression {
    Expression::new(0, ExpressionKind::Type(Box::new(ty(kind)), false), span())
}

pub fn node_map() -> TypeKind {
    TypeKind::Map(
        Box::new(type_expression(TypeKind::Int)),
        Box::new(type_expression(node())),
    )
}

/// A set of `element`s.
pub fn set_of(element: TypeKind) -> TypeKind {
    TypeKind::Set(Box::new(type_expression(element)))
}

/// `Keyed`, a class declaring its own `equals`, so matching it runs code
/// the program writes.
pub fn keyed() -> TypeKind {
    TypeKind::Custom("Keyed".to_string(), None)
}

fn keyed_class() -> TypeDefinition {
    let mut definition = class("Keyed", "k", TypeKind::Int);
    if let TypeDefinition::Class(class) = &mut definition {
        let equals = MethodInfo {
            params: Vec::new(),
            is_out_flags: Vec::new(),
            return_type: ty(TypeKind::Boolean),
            visibility: MemberVisibility::Public,
            is_constructor: false,
            is_abstract: false,
            is_static: false,
            attributes: Vec::new(),
        };
        class.methods.insert("equals".to_string(), equals);
    }
    definition
}

/// `Bag`, a class holding a set of `Keyed` values.
pub fn bag() -> TypeKind {
    TypeKind::Custom("Bag".to_string(), None)
}

/// `Tally`, a class holding a set of ints.
pub fn tally() -> TypeKind {
    TypeKind::Custom("Tally".to_string(), None)
}

pub fn definitions() -> HashMap<String, TypeDefinition> {
    HashMap::from([
        ("Node".to_string(), class("Node", "v", TypeKind::Int)),
        ("Holder".to_string(), class("Holder", "node", node())),
        ("Shelf".to_string(), class("Shelf", "items", int_list())),
        ("Registry".to_string(), class("Registry", "m", node_map())),
        ("Keyed".to_string(), keyed_class()),
        ("Bag".to_string(), class("Bag", "s", set_of(keyed()))),
        (
            "Tally".to_string(),
            class("Tally", "s", set_of(TypeKind::Int)),
        ),
    ])
}

/// A body whose local `i` has kind `kinds[i]`; local 0 is the return slot.
pub fn body_of(kinds: &[TypeKind], arg_count: usize, blocks: Vec<BasicBlockData>) -> Body {
    let mut body = Body::new(arg_count, span(), ExecutionModel::Cpu);
    for kind in kinds {
        body.new_local(LocalDecl::new(ty(kind.clone()), span()));
    }
    body.basic_blocks = blocks;
    body.rc_classifier = RcClassifier::new(Rc::new(definitions()));
    body
}

pub fn local(index: usize) -> Place {
    Place::new(Local(index))
}

pub fn field(index: usize, at: usize) -> Place {
    Place {
        local: Local(index),
        projection: vec![PlaceElem::Field(at)],
    }
}

pub fn statement(kind: StatementKind) -> Statement {
    Statement { kind, span: span() }
}

pub fn store(dest: Place, source: Place) -> Statement {
    statement(StatementKind::Assign(
        dest,
        Rvalue::Use(Operand::Copy(source)),
    ))
}

pub fn block(statements: Vec<Statement>, kind: TerminatorKind) -> BasicBlockData {
    BasicBlockData {
        statements,
        terminator: Some(Terminator { kind, span: span() }),
        is_cleanup: false,
    }
}

pub fn returning(statements: Vec<Statement>) -> BasicBlockData {
    block(statements, TerminatorKind::Return)
}

pub fn function(name: &str) -> Symbol {
    Symbol::declared_function(&ModuleId::Program, name)
}

/// A call of `func` handed `args`, writing `destination` and returning to
/// block `target`.
pub fn call_into(
    func: Operand,
    args: Vec<Operand>,
    destination: Place,
    target: usize,
) -> TerminatorKind {
    TerminatorKind::Call {
        func,
        args,
        arg_modes: Vec::new(),
        arg_handles: Vec::new(),
        destination,
        target: Some(BasicBlock(target)),
    }
}

/// A call of `func` handed `args`, returning to block 1.
pub fn call_of(func: Operand, args: Vec<Operand>) -> TerminatorKind {
    call_into(func, args, local(0), 1)
}

/// A call of the function of the program `name`, returning to block 1.
pub fn call(name: &str, args: Vec<Operand>) -> TerminatorKind {
    call_returning_to(name, args, 1)
}

/// A call of the function of the program `name`, returning to `target`.
pub fn call_returning_to(name: &str, args: Vec<Operand>, target: usize) -> TerminatorKind {
    call_into(
        Operand::function(function(name), span()),
        args,
        local(0),
        target,
    )
}

/// A call of the runtime entry point `name`, returning to block 1.
pub fn runtime_call(name: &str, args: Vec<Operand>) -> TerminatorKind {
    call_of(Operand::function(Symbol::runtime(name), span()), args)
}

/// A body that makes `terminator` its one call, then returns.
pub fn calling(kinds: &[TypeKind], arg_count: usize, terminator: TerminatorKind) -> Body {
    body_of(
        kinds,
        arg_count,
        vec![block(Vec::new(), terminator), returning(Vec::new())],
    )
}

pub fn copy(place: Place) -> Operand {
    Operand::Copy(place)
}

/// `fn reset(self Holder)`: replaces the node its receiver holds.
pub fn field_writing_method() -> Body {
    let mut body = body_of(
        &[TypeKind::Void, holder(), node()],
        1,
        vec![returning(vec![store(field(1, 0), local(2))])],
    );
    body.receiver = Some(Local(1));
    body
}

/// `fn read(h Holder, n Node)`: does nothing with what it reads.
pub fn read_only_function() -> Body {
    body_of(
        &[TypeKind::Void, holder(), node()],
        2,
        vec![returning(Vec::new())],
    )
}

pub fn effects(program: &[(Symbol, Body)], phase: Phase) -> HeapEffects {
    let definitions = definitions();
    HeapEffects::of(
        program,
        Symbol::link_name,
        phase,
        &DropHookReach::new(&definitions),
    )
}

fn is_quiet(program: &[(Symbol, Body)], name: &str) -> bool {
    effects(program, Phase::Lowered).is_quiet_callee(&function(name))
}

#[test]
fn functions_calling_each_other_and_nothing_loud_stay_quiet() {
    let program = vec![
        (
            function("ping"),
            calling(
                &[TypeKind::Void, node()],
                1,
                call("pong", vec![copy(local(1))]),
            ),
        ),
        (
            function("pong"),
            calling(
                &[TypeKind::Void, node()],
                1,
                call("ping", vec![copy(local(1))]),
            ),
        ),
    ];
    assert!(is_quiet(&program, "ping"));
    assert!(is_quiet(&program, "pong"));
}

#[test]
fn a_store_through_a_class_field_is_loud() {
    let program = vec![(function("reset"), field_writing_method())];
    assert!(!is_quiet(&program, "reset"));
}

#[test]
fn a_function_calling_a_field_writing_method_is_loud() {
    let program = vec![
        (function("reset"), field_writing_method()),
        (
            function("outer"),
            calling(
                &[TypeKind::Void, holder()],
                1,
                call("reset", vec![copy(local(1))]),
            ),
        ),
        (function("read"), read_only_function()),
    ];
    assert!(!is_quiet(&program, "outer"));
    assert!(is_quiet(&program, "read"));
}

#[test]
fn a_call_to_a_symbol_with_no_body_is_loud() {
    let program = vec![(
        function("outer"),
        calling(&[TypeKind::Void], 0, call("elsewhere", Vec::new())),
    )];
    assert!(!is_quiet(&program, "outer"));
}

#[test]
fn a_virtual_call_is_loud() {
    let virtual_call = TerminatorKind::VirtualCall {
        vtable_slot: 0,
        args: vec![copy(local(1))],
        arg_modes: Vec::new(),
        param_types: Vec::new(),
        destination: local(0),
        target: Some(BasicBlock(1)),
    };
    let program = vec![(
        function("dispatch"),
        calling(&[TypeKind::Void, node()], 1, virtual_call),
    )];
    assert!(!is_quiet(&program, "dispatch"));
}

#[test]
fn a_call_through_a_closure_value_is_loud() {
    let closure = TypeKind::Identifier;
    let program = vec![(
        function("invoke"),
        calling(
            &[TypeKind::Void, closure],
            1,
            call_of(copy(local(1)), Vec::new()),
        ),
    )];
    assert!(!is_quiet(&program, "invoke"));
}

#[test]
fn a_runtime_writer_on_a_parameter_is_loud() {
    let program = vec![(
        function("wipe"),
        calling(
            &[TypeKind::Void, int_list()],
            1,
            runtime_call(rt::LIST_CLEAR, vec![copy(local(1))]),
        ),
    )];
    assert!(!is_quiet(&program, "wipe"));
}

#[test]
fn a_runtime_writer_on_a_list_the_body_owns_is_quiet() {
    let program = vec![(
        function("wipe_own"),
        calling(
            &[TypeKind::Void, int_list()],
            0,
            runtime_call(rt::LIST_CLEAR, vec![copy(local(1))]),
        ),
    )];
    assert!(is_quiet(&program, "wipe_own"));
}

#[test]
fn copy_on_write_and_a_count_question_are_quiet() {
    let program = vec![
        (
            function("unshare"),
            calling(
                &[TypeKind::Void, int_list()],
                1,
                runtime_call(rt::LIST_COW, vec![copy(local(1))]),
            ),
        ),
        (
            function("ask"),
            calling(
                &[TypeKind::Boolean, int_list()],
                1,
                runtime_call(rt::RC_IS_SHARED, vec![copy(local(1))]),
            ),
        ),
    ];
    assert!(is_quiet(&program, "unshare"));
    assert!(is_quiet(&program, "ask"));
}

#[test]
fn a_release_after_counting_is_quiet_unless_a_hook_may_run() {
    let release = |kind: TypeKind| {
        body_of(
            &[TypeKind::Void, kind],
            0,
            vec![returning(vec![statement(StatementKind::DecRef(local(1)))])],
        )
    };
    let unknown = TypeKind::Custom("Resource".to_string(), None);
    let program = vec![
        (function("plain"), release(node())),
        (function("hooked"), release(unknown)),
    ];
    let effects = effects(&program, Phase::Counted);
    assert!(effects.is_quiet_callee(&function("plain")));
    assert!(!effects.is_quiet_callee(&function("hooked")));
}

#[test]
fn stamping_marks_each_call_to_a_quiet_callee_by_its_block() {
    let mut program = vec![
        (function("reset"), field_writing_method()),
        (function("read"), read_only_function()),
        (
            function("main"),
            body_of(
                &[TypeKind::Void, holder()],
                0,
                vec![
                    block(
                        Vec::new(),
                        call("read", vec![copy(local(1)), copy(field(1, 0))]),
                    ),
                    block(
                        Vec::new(),
                        call_returning_to("reset", vec![copy(local(1))], 2),
                    ),
                    returning(Vec::new()),
                ],
            ),
        ),
    ];
    let definitions = definitions();
    stamp_quiet_calls(&mut program, &DropHookReach::new(&definitions));
    let main = &program[2].1;
    assert!(main.quiet_calls.contains(&BasicBlock(0)));
    assert!(!main.quiet_calls.contains(&BasicBlock(1)));
}
