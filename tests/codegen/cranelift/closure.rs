// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the drop thunk of a closure's environment.
//!
//! A closure whose environment holds a counted capture carries the address of
//! its environment's drop thunk in its header, so releasing the closure
//! releases the captures without static knowledge of their types at the
//! release site. The thunk is planned in the type facts like every other drop
//! thunk; whether one is emitted is what a wrong diff gets wrong: none for a
//! counted capture leaks it, while one where the header holds null is an
//! unreferenced symbol.
//!
//! The thunk is emitted into the object file, so these tests compile real MIR
//! bodies through the public backend and look for the exported symbol in the
//! artifact bytes.

use miri::ast::type_arguments::{TypeArgKind, TypeArgument};
use miri::ast::types::{FunctionTypeData, Type, TypeKind};
use miri::codegen::cranelift::{CraneliftBackend, CraneliftOptions};
use miri::codegen::Backend;
use miri::error::syntax::Span;
use miri::mir::symbol::{ClosureKind, Symbol, ThunkKind};
use miri::mir::type_facts::TypeFacts;
use miri::mir::{
    AggregateKind, BasicBlockData, Body, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
};
use std::collections::HashMap;

fn span() -> Span {
    Span::new(0, 0)
}

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

fn type_expr(kind: TypeKind) -> TypeArgument {
    TypeArgument::new(0, TypeArgKind::Type(Box::new(ty(kind)), false), span())
}

fn list_of(elem: TypeKind) -> TypeKind {
    TypeKind::List(Box::new(type_expr(elem)))
}

fn function_type() -> Type {
    ty(TypeKind::Function(Box::new(FunctionTypeData {
        generics: None,
        params: Vec::new(),
        return_type: None,
    })))
}

fn returning(mut body: Body) -> Body {
    let mut block = BasicBlockData::new(None);
    block.terminator = Some(Terminator::new(TerminatorKind::Return, span()));
    body.basic_blocks.push(block);
    body
}

/// The body of `closure`, whose env captures `captures`, in declaration
/// order.
///
/// A lambda takes its env pointer as the first argument, so the body has one
/// argument local ahead of the capture locals.
fn lambda_capturing(closure: &Symbol, captures: &[TypeKind]) -> Body {
    let mut body = Body::new(1, span(), ExecutionModel::Cpu);
    body.captures.closure = Some(closure.clone());
    body.local_decls
        .push(LocalDecl::new(ty(TypeKind::Void), span()));
    body.local_decls
        .push(LocalDecl::new(ty(TypeKind::RawPtr), span()));

    for capture in captures {
        let local = body.new_local(LocalDecl::new(ty(capture.clone()), span()));
        body.captures.env_capture_locals.push(local);
    }
    returning(body)
}

/// A body creating a value of the closure `closure`.
fn creating(closure: &Symbol) -> Body {
    let mut body = Body::new(0, span(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(function_type(), span()));
    let mut block = BasicBlockData::new(None);
    block.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(0)),
            Rvalue::Aggregate(
                AggregateKind::Closure(closure.clone(), function_type()),
                vec![Operand::Copy(Place::new(Local(0)))],
            ),
        ),
        span: span(),
    });
    body.basic_blocks.push(block);
    body
}

/// The bytes of the object the closure `id` capturing `captures` compiles to,
/// with the facts the pipeline would settle for it.
fn compiled_closure(id: usize, captures: &[TypeKind]) -> (Symbol, Vec<u8>) {
    let closure = Symbol::closure(ClosureKind::Lambda, id, std::iter::empty::<&Type>());
    let name = closure.link_name();
    let lambda = lambda_capturing(&closure, captures);
    let creator = creating(&closure);
    let facts = TypeFacts::new(
        HashMap::new(),
        HashMap::new(),
        Default::default(),
        [&creator, &lambda],
    )
    .and_then(|facts| facts.with_closure_envs([&creator, &lambda]))
    .expect("the facts settle");
    let mut backend = CraneliftBackend::new().expect("host backend");
    backend.set_type_facts(facts);
    let artifact = backend
        .compile(&[(name.as_str(), &lambda)], &CraneliftOptions::default())
        .unwrap_or_else(|e| panic!("compiling {name} failed: {e:?}"));
    (closure, artifact.bytes)
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}

/// Whether the drop thunk of the environment of closure `id` capturing
/// `captures` is emitted.
fn emits_environment_thunk(id: usize, captures: &[TypeKind]) -> bool {
    let (closure, bytes) = compiled_closure(id, captures);
    let thunk = Symbol::environment_thunk(ThunkKind::Drop, &closure).link_name();
    contains(&bytes, thunk.as_bytes())
}

#[test]
fn test_a_string_capture_gets_an_environment_thunk() {
    assert!(emits_environment_thunk(1, &[TypeKind::String]));
}

#[test]
fn test_a_collection_capture_gets_an_environment_thunk() {
    assert!(emits_environment_thunk(2, &[list_of(TypeKind::Int)]));
}

#[test]
fn test_a_class_capture_gets_an_environment_thunk() {
    assert!(emits_environment_thunk(
        3,
        &[TypeKind::Custom("Counter".to_string(), None)]
    ));
}

#[test]
fn test_scalar_only_captures_get_no_environment_thunk() {
    // Scalars are copied into the env; there is nothing to release, and the
    // header's drop word is null instead of this thunk's address.
    assert!(!emits_environment_thunk(
        4,
        &[TypeKind::Int, TypeKind::Boolean]
    ));
}

#[test]
fn test_a_lambda_with_no_captures_gets_no_environment_thunk() {
    assert!(!emits_environment_thunk(5, &[]));
}

#[test]
fn test_one_counted_capture_among_scalars_still_gets_an_environment_thunk() {
    // The decision is "any capture is counted", not "all of them are" — a
    // mixed env must still be released, or the String leaks.
    assert!(emits_environment_thunk(
        6,
        &[TypeKind::Int, TypeKind::String, TypeKind::Boolean]
    ));
}

/// Three counted captures at different env offsets compile into one thunk
/// that loads each of their slots. That each is released at run time is
/// proven by `a_closure_releases_a_resource_at_every_capture_position`.
#[test]
fn test_several_counted_captures_compile_into_one_environment_thunk() {
    assert!(emits_environment_thunk(
        7,
        &[TypeKind::String, list_of(TypeKind::Int), TypeKind::String]
    ));
}

#[test]
fn test_no_closure_destructor_symbol_is_emitted() {
    // A closure's environment is released by its drop thunk alone: no
    // per-closure destructor is emitted beside it.
    let (_, bytes) = compiled_closure(8, &[TypeKind::String, list_of(TypeKind::Int)]);
    assert!(!contains(&bytes, b"$dtor"));
}
