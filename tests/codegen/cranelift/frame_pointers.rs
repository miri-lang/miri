// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A profile build keeps a frame-pointer record in every function, so a
//! profiler that walks frame pointers sees each caller.
//!
//! AArch64 leaves the frame out of a leaf function that makes no calls and
//! uses no stack, which drops the leaf's caller from the walked stack; the
//! leaf below is that case. x86-64 sets the frame up regardless, and is
//! checked so a profile build stays correct on both targets.

use cranelift_object::object::{File, Object, ObjectSection, ObjectSymbol};
use miri::ast::types::{Type, TypeKind};
use miri::codegen::cranelift::{CraneliftBackend, CraneliftOptions, OptLevel};
use miri::codegen::Backend;
use miri::error::syntax::Span;
use miri::mir::{
    BasicBlockData, Body, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use target_lexicon::Triple;

const LEAF: &str = "leaf";

const AARCH64_TARGETS: [&str; 2] = ["aarch64-unknown-linux-gnu", "aarch64-apple-darwin"];
const X86_64_TARGETS: [&str; 2] = ["x86_64-unknown-linux-gnu", "x86_64-apple-darwin"];

/// `stp x29, x30, [sp, #-16]!` then `mov x29, sp`, little-endian.
const AARCH64_FRAME_SETUP: [u8; 8] = [0xfd, 0x7b, 0xbf, 0xa9, 0xfd, 0x03, 0x00, 0x91];
/// `push rbp` then `mov rbp, rsp`.
const X86_64_FRAME_SETUP: [u8; 4] = [0x55, 0x48, 0x89, 0xe5];

fn span() -> Span {
    Span::new(0, 0)
}

fn local(index: usize) -> Place {
    Place {
        local: Local(index),
        projection: vec![],
    }
}

/// `fn leaf(value int) int` returning its argument: no calls, no stack.
fn leaf_body() -> Body {
    let int = Type::new(TypeKind::Int, span());
    let mut body = Body::new(1, span(), ExecutionModel::Cpu);
    body.local_decls.push(LocalDecl::new(int.clone(), span()));
    body.local_decls.push(LocalDecl::new(int, span()));
    let mut block = BasicBlockData::new(None);
    block.statements.push(Statement {
        kind: StatementKind::Assign(local(0), Rvalue::Use(Operand::Copy(local(1)))),
        span: span(),
    });
    block.terminator = Some(Terminator::new(TerminatorKind::Return, span()));
    body.basic_blocks.push(block);
    body
}

fn profile_options() -> CraneliftOptions {
    CraneliftOptions {
        opt_level: OptLevel::Speed,
        runtime_checks: false,
        preserve_frame_pointers: true,
        ..CraneliftOptions::default()
    }
}

/// The machine code the leaf compiles to for `target` under `options`.
fn leaf_code(target: &str, options: &CraneliftOptions) -> Vec<u8> {
    let triple: Triple = target.parse().expect("the target triple should parse");
    let backend = CraneliftBackend::for_target(triple).expect("the backend should accept it");
    let body = leaf_body();
    let artifact = backend
        .compile(&[(LEAF, &body)], options)
        .expect("the leaf should compile");
    let object = File::parse(&*artifact.bytes).expect("the artifact should be an object file");
    let symbol = object
        .symbols()
        .find(|symbol| matches!(symbol.name(), Ok(name) if name.trim_start_matches('_') == LEAF))
        .expect("the object should define the leaf");
    let section_index = symbol
        .section_index()
        .expect("the leaf should be in a section");
    let section = object
        .section_by_index(section_index)
        .expect("the leaf's section should exist");
    let data = section.data().expect("the section should have data");
    let start = usize::try_from(symbol.address() - section.address()).expect("offset fits");
    data[start..].to_vec()
}

fn assert_starts_with_frame_setup(target: &str, options: &CraneliftOptions, setup: &[u8]) {
    let code = leaf_code(target, options);
    assert!(
        code.starts_with(setup),
        "the leaf for {target} should start with frame setup {setup:02x?}, starts with {:02x?}",
        &code[..setup.len().min(code.len())]
    );
}

#[test]
fn a_profile_build_sets_up_a_frame_in_an_aarch64_leaf() {
    for target in AARCH64_TARGETS {
        assert_starts_with_frame_setup(target, &profile_options(), &AARCH64_FRAME_SETUP);
    }
}

#[test]
fn a_profile_build_sets_up_a_frame_in_an_x86_64_leaf() {
    for target in X86_64_TARGETS {
        assert_starts_with_frame_setup(target, &profile_options(), &X86_64_FRAME_SETUP);
    }
}

/// An unoptimized build with frame pointers asks for everything the backend's
/// cached default target does except the frame pointers, so it is the case a
/// reused default would get wrong.
#[test]
fn frame_pointers_reach_an_unoptimized_build() {
    let options = CraneliftOptions {
        preserve_frame_pointers: true,
        ..CraneliftOptions::default()
    };
    for target in AARCH64_TARGETS {
        assert_starts_with_frame_setup(target, &options, &AARCH64_FRAME_SETUP);
    }
}

/// Without the flag the same AArch64 leaf has no frame, which is what makes
/// the assertions above say something.
#[test]
fn a_release_build_leaves_the_frame_out_of_an_aarch64_leaf() {
    let release = CraneliftOptions {
        preserve_frame_pointers: false,
        ..profile_options()
    };
    for target in AARCH64_TARGETS {
        let code = leaf_code(target, &release);
        assert!(
            !code.starts_with(&AARCH64_FRAME_SETUP),
            "the release leaf for {target} unexpectedly sets up a frame"
        );
    }
}
