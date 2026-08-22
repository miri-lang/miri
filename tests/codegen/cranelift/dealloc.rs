// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Codegen coverage for `StatementKind::Dealloc`.
//!
//! Nothing in the compiler constructs `Dealloc` — it is a seam held open for a
//! future pass that can prove unique ownership. That left `translate_dealloc`
//! written but never executed, so a defect in it could not surface anywhere.
//! These tests drive it from a hand-built body, which is the only way to reach
//! it today.
//!
//! What is *not* covered: whether the immortal guard inside `translate_dealloc`
//! takes the correct branch at run time. Observing that needs a program that
//! reaches a `Dealloc` holding an immortal value, and no program can construct
//! one until a pass emits the variant. Until then the guard is held to matching
//! the one `emit_decref_value` applies at the same point, and a pass that starts
//! emitting `Dealloc` owes this file an execution test.

use miri::ast::types::{Type, TypeKind};
use miri::codegen::backend::Backend;
use miri::codegen::cranelift::{CraneliftBackend, CraneliftOptions};
use miri::error::syntax::Span;
use miri::mir::{
    BasicBlockData, Body, ExecutionModel, Local, LocalDecl, Place, Statement, StatementKind,
    Terminator, TerminatorKind,
};

fn span() -> Span {
    Span::new(0, 0)
}

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

/// A body holding a single `Dealloc` on a managed local, which is the shape a
/// uniqueness-proving pass would emit in place of a `DecRef`.
fn body_with_dealloc() -> Body {
    let mut body = Body::new(0, span(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ty(TypeKind::Void), span()));
    body.new_local(LocalDecl::new(ty(TypeKind::String), span()));

    let mut block = BasicBlockData::new(None);
    for kind in [
        StatementKind::StorageLive(Place::new(Local(1))),
        StatementKind::Dealloc(Place::new(Local(1))),
        StatementKind::StorageDead(Place::new(Local(1))),
    ] {
        block.statements.push(Statement { kind, span: span() });
    }
    block.terminator = Some(Terminator {
        kind: TerminatorKind::Return,
        span: span(),
    });
    body.basic_blocks.push(block);
    body
}

#[test]
fn dealloc_translates_to_object_code() {
    let body = body_with_dealloc();
    let backend = CraneliftBackend::new().expect("host backend");

    let artifact = backend
        .compile(&[("test_dealloc", &body)], &CraneliftOptions::default())
        .expect("Dealloc should translate");

    assert!(
        !artifact.bytes.is_empty(),
        "translating a Dealloc should produce object bytes"
    );
}
