// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for closure-destructor emission.
//!
//! A closure that captures managed values carries a `__dtor_{lambda}` pointer
//! in its env so the runtime can DecRef the captures when the closure's RC
//! reaches zero, without static knowledge of the capture types at the drop
//! site. The decision of *whether* to emit that destructor is what a wrong diff
//! gets wrong: emitting none for a managed capture leaks (or worse, drops the
//! captures through a null pointer), while emitting one where the call site
//! passes a null dtor pointer leaves an unreferenced symbol behind.
//!
//! The destructor is emitted into the object file, so these tests compile real
//! MIR bodies through the public backend and look for the exported symbol in
//! the artifact bytes.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{Type, TypeKind};
use miri::codegen::cranelift::{CraneliftBackend, CraneliftOptions};
use miri::codegen::Backend;
use miri::error::syntax::Span;
use miri::mir::{BasicBlockData, Body, ExecutionModel, LocalDecl, Terminator, TerminatorKind};

fn span() -> Span {
    Span::new(0, 0)
}

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

fn type_expr(kind: TypeKind) -> Expression {
    Expression::new(0, ExpressionKind::Type(Box::new(ty(kind)), false), span())
}

fn list_of(elem: TypeKind) -> TypeKind {
    TypeKind::List(Box::new(type_expr(elem)))
}

/// A lambda body whose env captures `captures`, in declaration order.
///
/// A lambda takes its env pointer as the first argument, so the body has one
/// argument local ahead of the capture locals.
fn lambda_capturing(captures: &[TypeKind]) -> Body {
    let mut body = Body::new(1, span(), ExecutionModel::Cpu);
    body.local_decls
        .push(LocalDecl::new(ty(TypeKind::Void), span()));
    body.local_decls
        .push(LocalDecl::new(ty(TypeKind::RawPtr), span()));

    for capture in captures {
        let local = body.new_local(LocalDecl::new(ty(capture.clone()), span()));
        body.env_capture_locals.push(local);
    }

    let mut block = BasicBlockData::new(None);
    block.terminator = Some(Terminator::new(TerminatorKind::Return, span()));
    body.basic_blocks.push(block);
    body
}

/// Compile `body` under `name` and report whether `__dtor_{name}` was emitted.
fn emits_destructor_for(name: &str, body: &Body) -> bool {
    let backend = CraneliftBackend::new().expect("host backend");
    let artifact = backend
        .compile(&[(name, body)], &CraneliftOptions::default())
        .unwrap_or_else(|e| panic!("compiling {name} failed: {e:?}"));

    let symbol = format!("__dtor_{name}").into_bytes();
    artifact
        .bytes
        .windows(symbol.len())
        .any(|window| window == symbol)
}

#[test]
fn test_a_string_capture_gets_a_destructor() {
    let body = lambda_capturing(&[TypeKind::String]);
    assert!(emits_destructor_for("lambda_string", &body));
}

#[test]
fn test_a_collection_capture_gets_a_destructor() {
    let body = lambda_capturing(&[list_of(TypeKind::Int)]);
    assert!(emits_destructor_for("lambda_list", &body));
}

#[test]
fn test_a_class_capture_gets_a_destructor() {
    let body = lambda_capturing(&[TypeKind::Custom("Counter".to_string(), None)]);
    assert!(emits_destructor_for("lambda_class", &body));
}

#[test]
fn test_scalar_only_captures_get_no_destructor() {
    // Scalars are copied into the env; there is nothing to DecRef, and the call
    // site stores a null dtor pointer instead of taking this symbol's address.
    let body = lambda_capturing(&[TypeKind::Int, TypeKind::Boolean]);
    assert!(!emits_destructor_for("lambda_scalars", &body));
}

#[test]
fn test_a_lambda_with_no_captures_gets_no_destructor() {
    let body = lambda_capturing(&[]);
    assert!(!emits_destructor_for("lambda_empty", &body));
}

#[test]
fn test_one_managed_capture_among_scalars_still_gets_a_destructor() {
    // The decision is "any capture is managed", not "all of them are" — a mixed
    // env must still be swept, or the String leaks.
    let body = lambda_capturing(&[TypeKind::Int, TypeKind::String, TypeKind::Boolean]);
    assert!(emits_destructor_for("lambda_mixed", &body));
}

#[test]
fn test_the_destructor_is_named_after_its_lambda() {
    // The call site resolves the destructor by name (`__dtor_{lambda}`), so a
    // rename on either side is a link error rather than a silent miss.
    let body = lambda_capturing(&[TypeKind::String]);
    assert!(emits_destructor_for("lambda_named_target", &body));
    assert!(!emits_destructor_for(
        "lambda_named_target",
        &lambda_capturing(&[TypeKind::Int])
    ));
}

#[test]
fn test_every_managed_capture_is_swept_not_just_the_first() {
    // Two managed captures at different env offsets; the destructor must load
    // both slots, so compilation has to succeed with the full capture list.
    let body = lambda_capturing(&[TypeKind::String, list_of(TypeKind::Int), TypeKind::String]);
    assert!(emits_destructor_for("lambda_many", &body));
}
