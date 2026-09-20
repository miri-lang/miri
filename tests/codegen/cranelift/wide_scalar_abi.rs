// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A function whose signature carries a 128-bit scalar must compile for every
//! target the compiler can emit, not only for the one the developer happens to
//! be sitting at.
//!
//! The x86-64 ABIs describe several ways to hand over a 128-bit integer, so
//! Cranelift refuses to guess: it aborts the whole process the moment such a
//! parameter or return value reaches signature lowering unless the backend has
//! asked for the LLVM lowering by name. AArch64 has no such ambiguity and
//! lowers the same signature without being asked, which is why a host-only test
//! of `i128` says nothing about what an x86-64 build does. These targets are
//! therefore named explicitly.

use miri::ast::types::{Type, TypeKind};
use miri::codegen::cranelift::{CraneliftBackend, CraneliftOptions};
use miri::codegen::Backend;
use miri::error::syntax::Span;
use miri::mir::{
    BasicBlockData, Body, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use target_lexicon::Triple;

/// The targets a 128-bit signature has to survive. x86-64 is the one that
/// aborts when the backend stays silent about its 128-bit lowering; the
/// AArch64 entries keep the developer's own machine in the same test.
const TARGETS: [&str; 4] = [
    "x86_64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
];

fn span() -> Span {
    Span::new(0, 0)
}

/// `fn identity(value: i128) -> i128` — the smallest body that puts a 128-bit
/// scalar in both halves of a signature.
fn identity_over_wide_scalar(kind: TypeKind) -> Body {
    let wide = Type::new(kind, span());
    let mut body = Body::new(1, span(), ExecutionModel::Cpu);
    body.local_decls.push(LocalDecl::new(wide.clone(), span()));
    body.local_decls.push(LocalDecl::new(wide, span()));

    let mut block = BasicBlockData::new(None);
    block.statements.push(Statement {
        kind: StatementKind::Assign(
            Place {
                local: Local(0),
                projection: vec![],
            },
            Rvalue::Use(Operand::Copy(Place {
                local: Local(1),
                projection: vec![],
            })),
        ),
        span: span(),
    });
    block.terminator = Some(Terminator::new(TerminatorKind::Return, span()));
    body.basic_blocks.push(block);

    body
}

fn assert_compiles_for_every_target(kind: TypeKind) {
    for name in TARGETS {
        let triple: Triple = name.parse().expect("the target triple should parse");
        let backend =
            CraneliftBackend::for_target(triple).expect("the backend should accept the target");
        let body = identity_over_wide_scalar(kind.clone());

        let compiled = backend.compile(&[("identity", &body)], &CraneliftOptions::default());

        assert!(
            compiled.is_ok(),
            "a {} signature did not compile for {}: {:?}",
            kind,
            name,
            compiled.err()
        );
    }
}

#[test]
fn a_signed_128_bit_signature_compiles_for_every_target() {
    assert_compiles_for_every_target(TypeKind::I128);
}

#[test]
fn an_unsigned_128_bit_signature_compiles_for_every_target() {
    assert_compiles_for_every_target(TypeKind::U128);
}
