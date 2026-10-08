// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The instantiations a drop thunk is emitted for are every concrete one a
//! compiled body holds a value of, closed over what their fields store — not
//! only the ones whose method bodies were lowered.

use miri::ast::factory::type_expr_non_null;
use miri::ast::types::{Type, TypeDeclarationKind, TypeKind};
use miri::ast::MemberVisibility;
use miri::error::syntax::Span;
use miri::mir::body::{Body, ExecutionModel, LocalDecl};
use miri::mir::type_facts::{
    ClassDefinition, DropInstantiationRefusal, GenericDefinition, TypeDefinition, TypeFacts,
};
use miri::mir::{AggregateKind, BasicBlockData, Local, Place, Rvalue, Statement, StatementKind};
use miri::type_checker::context::FieldInfo;
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn parameter(name: &str) -> Type {
    ty(TypeKind::Generic(
        name.to_string(),
        None,
        TypeDeclarationKind::None,
    ))
}

fn instance(name: &str, args: Vec<Type>) -> Type {
    ty(TypeKind::Custom(
        name.to_string(),
        Some(args.into_iter().map(type_expr_non_null).collect()),
    ))
}

/// `class {name}<T>` holding one field of type `field`.
fn generic_class(name: &str, field: Type) -> TypeDefinition {
    TypeDefinition::Class(ClassDefinition {
        name: name.to_string(),
        generics: Some(vec![GenericDefinition {
            name: "T".to_string(),
            constraint: None,
            kind: TypeDeclarationKind::None,
        }]),
        base_class: None,
        base_class_args: None,
        traits: Vec::new(),
        trait_args: HashMap::new(),
        fields: vec![(
            "value".to_string(),
            FieldInfo {
                ty: field,
                mutable: true,
                visibility: MemberVisibility::Public,
                initializer: None,
            },
        )],
        methods: BTreeMap::new(),
        module: String::new(),
        is_abstract: false,
        runtime_settled_methods: BTreeSet::new(),
    })
}

/// `Box<T>` holding a `T`, and `Outer<T>` holding a `Box<T>`.
fn definitions() -> HashMap<String, TypeDefinition> {
    HashMap::from([
        ("Box".to_string(), generic_class("Box", parameter("T"))),
        (
            "Outer".to_string(),
            generic_class("Outer", instance("Box", vec![parameter("T")])),
        ),
    ])
}

/// A body holding one local of each type in `held`.
fn body_holding(held: Vec<Type>) -> Body {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    for held_ty in held {
        body.new_local(LocalDecl::new(held_ty, Span::default()));
    }
    body
}

fn string() -> Type {
    ty(TypeKind::String)
}

#[test]
fn an_instantiation_a_body_holds_gets_a_drop_thunk_without_a_lowered_method() {
    let body = body_holding(vec![instance("Box", vec![string()])]);
    let facts = TypeFacts::new(definitions(), HashMap::new(), Default::default(), [&body]).unwrap();

    assert!(facts.is_drop_instantiation("Box", &[string()]));
}

#[test]
fn an_instantiation_a_field_stores_is_closed_over() {
    let body = body_holding(vec![instance("Outer", vec![string()])]);
    let facts = TypeFacts::new(definitions(), HashMap::new(), Default::default(), [&body]).unwrap();

    assert!(facts.is_drop_instantiation("Outer", &[string()]));
    assert!(
        facts.is_drop_instantiation("Box", &[string()]),
        "the `Box<String>` an `Outer<String>` stores is released by its own thunk"
    );
}

#[test]
fn an_instantiation_nested_in_an_optional_is_found() {
    let body = body_holding(vec![ty(TypeKind::Option(Box::new(instance(
        "Box",
        vec![string()],
    ))))]);
    let facts = TypeFacts::new(definitions(), HashMap::new(), Default::default(), [&body]).unwrap();

    assert!(facts.is_drop_instantiation("Box", &[string()]));
}

#[test]
fn an_instantiation_at_an_open_parameter_gets_no_thunk() {
    let body = body_holding(vec![instance("Box", vec![parameter("T")])]);
    let facts = TypeFacts::new(definitions(), HashMap::new(), Default::default(), [&body]).unwrap();

    assert!(facts.drop_instantiations_of("Box").is_empty());
}

#[test]
fn a_registered_instantiation_keeps_its_thunk() {
    let registry = HashMap::from([("Box".to_string(), vec![vec![string()]])]);
    let facts = TypeFacts::new(definitions(), registry, Default::default(), []).unwrap();

    assert!(facts.is_drop_instantiation("Box", &[string()]));
    assert!(!facts.is_drop_instantiation("Box", &[ty(TypeKind::Int)]));
}

#[test]
fn an_instantiation_only_a_closure_captures_gets_a_drop_thunk() {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    body.closure_capture_types
        .insert(Local(0), vec![instance("Box", vec![string()])]);
    let facts = TypeFacts::new(definitions(), HashMap::new(), Default::default(), [&body]).unwrap();

    assert!(facts.is_drop_instantiation("Box", &[string()]));
}

#[test]
fn an_instantiation_only_a_construction_names_gets_a_drop_thunk() {
    // The destination is typed as something else entirely, so only the
    // construction itself names `Box<String>`.
    let mut body = body_holding(vec![ty(TypeKind::Int)]);
    let mut block = BasicBlockData::new(None);
    block.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(0)),
            Rvalue::Aggregate(
                AggregateKind::Struct(instance("Box", vec![string()])),
                Vec::new(),
            ),
        ),
        span: Span::default(),
    });
    body.basic_blocks.push(block);
    let facts = TypeFacts::new(definitions(), HashMap::new(), Default::default(), [&body]).unwrap();

    assert!(facts.is_drop_instantiation("Box", &[string()]));
}

#[test]
fn a_field_nesting_its_own_type_deeper_is_refused_at_the_depth_bound() {
    // `Grow<T>` stores a `Grow<List<T>>`: every instantiation reaches a deeper
    // one, so the closure is refused once an instance passes the depth bound.
    let definitions = HashMap::from([
        ("Box".to_string(), generic_class("Box", parameter("T"))),
        (
            "Grow".to_string(),
            generic_class(
                "Grow",
                instance("Grow", vec![instance("Box", vec![parameter("T")])]),
            ),
        ),
    ]);
    let body = body_holding(vec![instance("Grow", vec![string()])]);
    let refusal = TypeFacts::new(definitions, HashMap::new(), Default::default(), [&body])
        .expect_err("an ever-deeper field has no finite set of drop thunks");

    let DropInstantiationRefusal::TooDeep { name, depth, .. } = refusal else {
        panic!("expected the depth bound to refuse, got {refusal:?}");
    };
    assert_eq!(name, "Grow");
    assert_eq!(depth, 33);
}

/// `class Duo<A, B>` holding one `value A`.
fn duo() -> TypeDefinition {
    let TypeDefinition::Class(mut def) = generic_class("Duo", parameter("A")) else {
        panic!("`generic_class` builds a class");
    };
    def.generics = Some(
        ["A", "B"]
            .iter()
            .map(|name| GenericDefinition {
                name: name.to_string(),
                constraint: None,
                kind: TypeDeclarationKind::None,
            })
            .collect(),
    );
    TypeDefinition::Class(def)
}

#[test]
fn an_unbound_argument_spelled_as_a_bare_name_names_the_same_drop_function() {
    // Inference writes an argument it left unbound either as the open
    // parameter or as its bare name; both are one instantiation.
    let bare_b = ty(TypeKind::Custom("B".to_string(), None));
    let body = body_holding(vec![instance("Duo", vec![string(), bare_b])]);
    let definitions = HashMap::from([("Duo".to_string(), duo())]);
    let facts = TypeFacts::new(definitions, HashMap::new(), Default::default(), [&body]).unwrap();

    assert!(facts.is_drop_instantiation("Duo", &[string(), parameter("B")]));
}
