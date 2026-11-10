// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What a closure's environment holds and what releasing it releases: one
//! fact per closure body, keyed by the closure the body records it is the
//! body of, and read by the code that builds the environment, the loads that
//! open the body and the thunk that drops it.

use miri::ast::type_arguments::{TypeArgKind, TypeArgument};
use miri::ast::types::{FunctionTypeData, Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::body::{Body, ExecutionModel, LocalDecl};
use miri::mir::symbol::{ClosureKind, Symbol};
use miri::mir::type_facts::{
    CaptureHeld, CountedSlot, DropInstantiationRefusal, DroppedMembers, ScalarKind, TypeFacts,
};
use std::collections::HashMap;

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn type_expr(kind: TypeKind) -> TypeArgument {
    TypeArgument::new(
        0,
        TypeArgKind::Type(Box::new(ty(kind)), false),
        Span::default(),
    )
}

fn list_of_int() -> TypeKind {
    TypeKind::List(Box::new(type_expr(TypeKind::Int)))
}

/// A tuple of three scalars: a bitwise value held as its 24 bytes.
fn triple() -> TypeKind {
    TypeKind::Tuple(vec![
        type_expr(TypeKind::Int),
        type_expr(TypeKind::Int),
        type_expr(TypeKind::Int),
    ])
}

fn function_type() -> Type {
    ty(TypeKind::Function(Box::new(FunctionTypeData {
        generics: None,
        params: Vec::new(),
        return_type: None,
    })))
}

/// A body whose environment captures `captures`, in order, after its return
/// slot and its environment pointer.
fn capturing(captures: &[TypeKind]) -> Body {
    let mut body = Body::new(1, Span::default(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ty(TypeKind::Int), Span::default()));
    body.new_local(LocalDecl::new(ty(TypeKind::RawPtr), Span::default()));
    for capture in captures {
        let local = body.new_local(LocalDecl::new(ty(capture.clone()), Span::default()));
        body.captures.env_capture_locals.push(local);
    }
    body
}

/// The body of `closure`, capturing `captures`.
fn body_of(closure: &Symbol, captures: &[TypeKind]) -> Body {
    let mut body = capturing(captures);
    body.captures.closure = Some(closure.clone());
    body
}

/// The facts settled over `bodies`.
fn facts_over(bodies: &[&Body]) -> Result<TypeFacts, DropInstantiationRefusal> {
    TypeFacts::new(
        HashMap::new(),
        HashMap::new(),
        Default::default(),
        bodies.iter().copied(),
    )?
    .with_closure_envs(bodies.iter().copied())
}

/// The facts over a program holding only the body of `closure`, capturing
/// `captures`: no body builds a value of it.
fn facts_for(closure: &Symbol, captures: &[TypeKind]) -> TypeFacts {
    facts_over(&[&body_of(closure, captures)]).expect("the facts settle")
}

fn lambda(id: usize) -> Symbol {
    Symbol::closure(ClosureKind::Lambda, id, std::iter::empty::<&Type>())
}

#[test]
fn an_environment_releases_only_its_counted_captures() {
    let closure = lambda(1);
    let captures = [
        TypeKind::Int,
        TypeKind::String,
        TypeKind::Float,
        list_of_int(),
    ];
    let facts = facts_for(&closure, &captures);
    let env = facts.closure_env(&closure).expect("the closure has an env");

    assert_eq!(
        env.plan.members,
        DroppedMembers::Fields(vec![
            CountedSlot {
                index: 1,
                ty: ty(TypeKind::String),
            },
            CountedSlot {
                index: 3,
                ty: ty(list_of_int()),
            },
        ])
    );
    assert_eq!(env.plan.hook, None);
    assert!(env.plan.releases_anything());
}

#[test]
fn each_capture_takes_whole_words_past_the_header_at_its_own_width() {
    let closure = lambda(2);
    let facts = facts_for(
        &closure,
        &[TypeKind::Int, TypeKind::I128, TypeKind::Boolean],
    );
    let env = facts.closure_env(&closure).expect("the closure has an env");

    let offsets: Vec<u32> = env.layout.slots.iter().map(|slot| slot.offset).collect();
    let held: Vec<&CaptureHeld> = env.layout.slots.iter().map(|slot| &slot.held).collect();
    assert_eq!(offsets, vec![0, 8, 24]);
    assert_eq!(
        held,
        vec![
            &CaptureHeld::Scalar(ScalarKind::Signed(8)),
            &CaptureHeld::Scalar(ScalarKind::Signed(16)),
            &CaptureHeld::Scalar(ScalarKind::Boolean),
        ]
    );
    assert_eq!(env.layout.captures_bytes, 32);
}

#[test]
fn a_bitwise_capture_is_held_as_its_bytes_and_the_next_capture_follows_them() {
    let closure = lambda(3);
    let facts = facts_for(&closure, &[triple(), TypeKind::String]);
    let env = facts.closure_env(&closure).expect("the closure has an env");

    let [bytes, word] = env.layout.slots.as_slice() else {
        panic!("two slots expected, found {:?}", env.layout.slots);
    };
    let CaptureHeld::Bytes(layout) = &bytes.held else {
        panic!("the triple is held as {:?}", bytes.held);
    };
    assert_eq!((bytes.offset, layout.size), (0, 24));
    assert_eq!((word.offset, &word.held), (24, &CaptureHeld::Word));
    assert_eq!(env.layout.captures_bytes, 32);
    assert_eq!(
        env.plan.members,
        DroppedMembers::Fields(vec![CountedSlot {
            index: 1,
            ty: ty(TypeKind::String),
        }])
    );
}

#[test]
fn an_environment_whose_captures_reach_no_closure_does_not_recurse() {
    let closure = lambda(4);
    let facts = facts_for(&closure, &[TypeKind::String, list_of_int()]);
    let env = facts.closure_env(&closure).expect("the closure has an env");

    assert!(!env.plan.may_recurse);
}

#[test]
fn an_environment_capturing_a_closure_may_recurse() {
    let closure = lambda(5);
    let facts = facts_for(&closure, &[function_type().kind]);
    let env = facts.closure_env(&closure).expect("the closure has an env");

    assert!(env.plan.may_recurse);
}

#[test]
fn a_capture_free_closure_has_an_empty_environment_that_releases_nothing() {
    let closure = lambda(6);
    let facts = facts_for(&closure, &[]);
    let env = facts.closure_env(&closure).expect("the closure has an env");

    assert!(!env.plan.releases_anything());
    assert!(!env.plan.may_recurse);
    assert!(env.layout.slots.is_empty());
    assert_eq!(env.layout.captures_bytes, 0);
}

#[test]
fn a_scalar_only_environment_releases_nothing() {
    let closure = lambda(7);
    let facts = facts_for(&closure, &[TypeKind::Int, TypeKind::Boolean]);
    let env = facts.closure_env(&closure).expect("the closure has an env");

    assert!(!env.plan.releases_anything());
}

/// A closure whose value no compiled body builds — its creator removed by an
/// optimisation — still has an environment, because its body is still
/// emitted and opens one.
#[test]
fn a_closure_body_no_value_is_built_of_still_has_an_environment() {
    let closure = lambda(8);
    let body = body_of(&closure, &[TypeKind::String]);
    let facts = facts_over(&[&body]).expect("the facts settle");

    let env = facts.closure_of_body(&body).expect("the body opens an env");
    assert_eq!(env.layout.slots.len(), 1);
    assert!(facts.closure_env(&lambda(9)).is_none());
}

#[test]
fn a_body_that_is_no_closure_has_no_environment() {
    let body = capturing(&[]);
    let facts = facts_over(&[&body]).expect("the facts settle");

    assert!(facts.closure_of_body(&body).is_none());
}

#[test]
fn two_instantiations_of_one_lambda_get_two_environments() {
    let at_int = Symbol::closure(ClosureKind::Lambda, 10, [&ty(TypeKind::Int)]);
    let at_string = Symbol::closure(ClosureKind::Lambda, 10, [&ty(TypeKind::String)]);
    let int_body = body_of(&at_int, &[TypeKind::Int]);
    let string_body = body_of(&at_string, &[TypeKind::String]);
    let facts = facts_over(&[&int_body, &string_body]).expect("the facts settle");

    let int_env = facts
        .closure_env(&at_int)
        .expect("the int instance has an env");
    let string_env = (facts.closure_env(&at_string)).expect("the String instance has an env");
    assert!(!int_env.plan.releases_anything());
    assert!(string_env.plan.releases_anything());
    assert_eq!(facts.closure_envs().count(), 2);
}

#[test]
fn a_body_reading_captures_without_naming_its_closure_is_refused() {
    let body = capturing(&[TypeKind::String]);

    let refusal = facts_over(&[&body]).expect_err("the body names no closure");
    assert!(
        matches!(
            &refusal,
            DropInstantiationRefusal::ClosureEnvironment(message)
                if message.contains("records no closure")
        ),
        "{refusal:?}"
    );
}
