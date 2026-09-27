// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The backend releases a generic instance only through a drop thunk the type
//! facts it was handed emit. From source, every instance a compiled body holds
//! is in those facts — the pipeline settles them over the same bodies it hands
//! the backend — so a release no thunk covers can only come from a pipeline
//! that settled them over different bodies. That mismatch surfaces as a
//! codegen error naming the instantiation, never as a fallback to the shared
//! thunk that would leak the managed field.

use miri::ast::types::{Type, TypeDeclarationKind, TypeKind};
use miri::ast::MemberVisibility;
use miri::codegen::backend::Backend;
use miri::codegen::cranelift::{CraneliftBackend, CraneliftOptions};
use miri::error::syntax::Span;
use miri::mir::type_facts::{
    ClassDefinition, FieldInfo, GenericDefinition, TypeDefinition, TypeFacts,
};
use miri::mir::{
    BasicBlockData, Body, ExecutionModel, Local, LocalDecl, Place, Statement, StatementKind,
    Terminator, TerminatorKind,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

/// `class Box<T>` holding one `value T`.
fn definitions() -> HashMap<String, TypeDefinition> {
    let parameter = ty(TypeKind::Generic(
        "T".to_string(),
        None,
        TypeDeclarationKind::None,
    ));
    HashMap::from([(
        "Box".to_string(),
        TypeDefinition::Class(ClassDefinition {
            name: "Box".to_string(),
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
                    ty: parameter,
                    mutable: true,
                    visibility: MemberVisibility::Public,
                },
            )],
            methods: BTreeMap::new(),
            module: String::new(),
            is_abstract: false,
            runtime_settled_methods: BTreeSet::new(),
        }),
    )])
}

/// A body releasing one local held at `Box<String>`.
fn body_releasing_a_box_of_string() -> Body {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ty(TypeKind::Void), Span::default()));
    let string_arg = miri::ast::factory::type_expr_non_null(ty(TypeKind::String));
    body.new_local(LocalDecl::new(
        ty(TypeKind::Custom("Box".to_string(), Some(vec![string_arg]))),
        Span::default(),
    ));
    let mut block = BasicBlockData::new(None);
    block.statements.push(Statement {
        kind: StatementKind::DecRef(Place::new(Local(1))),
        span: Span::default(),
    });
    block.terminator = Some(Terminator::new(TerminatorKind::Return, Span::default()));
    body.basic_blocks.push(block);
    body
}

fn compile(facts: TypeFacts, body: &Body) -> Result<(), String> {
    let mut backend = CraneliftBackend::new().map_err(|e| e.to_string())?;
    backend.set_type_facts(facts);
    backend
        .compile(&[("release_box", body)], &CraneliftOptions::default())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[test]
fn a_release_no_drop_thunk_covers_is_reported() {
    let body = body_releasing_a_box_of_string();
    // Settled over no bodies, the facts emit no thunk for `Box<String>`.
    let facts = TypeFacts::new(definitions(), HashMap::new(), Default::default(), []).unwrap();

    let error = compile(facts, &body).expect_err("the release has no thunk to call");

    assert!(
        error.contains("no drop thunk is emitted for"),
        "unexpected error: {error}"
    );
}

#[test]
fn a_release_the_settled_facts_cover_compiles() {
    let body = body_releasing_a_box_of_string();
    let facts = TypeFacts::new(definitions(), HashMap::new(), Default::default(), [&body]).unwrap();

    compile(facts, &body).expect("the facts settled over this body cover its release");
}
