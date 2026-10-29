// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every compiled function starts on a 64-byte cache line, on every target, so
//! where a hot loop falls against cache lines and fetch windows does not depend
//! on the size of the code placed before it.
//!
//! Three small functions are compiled together: unaligned, the second and
//! third would follow the first at whatever offset its size leaves.

use cranelift_object::object::{File, Object, ObjectSymbol, SymbolKind};
use miri::ast::types::{Type, TypeKind};
use miri::codegen::cranelift::{CraneliftBackend, CraneliftOptions, OptLevel};
use miri::codegen::Backend;
use miri::error::syntax::Span;
use miri::mir::{
    BasicBlockData, Body, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use target_lexicon::Triple;

const CACHE_LINE: u64 = 64;
const NAMES: [&str; 3] = ["first", "second", "third"];
const TARGETS: [&str; 4] = [
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "x86_64-apple-darwin",
];

fn span() -> Span {
    Span::new(0, 0)
}

fn local(index: usize) -> Place {
    Place {
        local: Local(index),
        projection: vec![],
    }
}

/// `fn (value int) int` returning its argument.
fn identity_body() -> Body {
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

/// The address of each of `NAMES` in the object compiled for `target`.
fn function_addresses(target: &str, options: &CraneliftOptions) -> Vec<(String, u64)> {
    let triple: Triple = target.parse().expect("the target triple should parse");
    let backend = CraneliftBackend::for_target(triple).expect("the backend should accept it");
    let body = identity_body();
    let functions: Vec<(&str, &Body)> = NAMES.iter().map(|name| (*name, &body)).collect();
    let artifact = backend
        .compile(&functions, options)
        .expect("the functions should compile");
    let object = File::parse(&*artifact.bytes).expect("the artifact should be an object file");
    object
        .symbols()
        .filter(|symbol| symbol.kind() == SymbolKind::Text)
        .filter_map(|symbol| {
            let name = symbol.name().ok()?.trim_start_matches('_').to_string();
            NAMES
                .contains(&name.as_str())
                .then(|| (name, symbol.address()))
        })
        .collect()
}

fn assert_functions_start_on_cache_lines(options: &CraneliftOptions) {
    for target in TARGETS {
        let addresses = function_addresses(target, options);
        assert_eq!(addresses.len(), NAMES.len(), "{target}: {addresses:?}");
        for (name, address) in addresses {
            assert_eq!(
                address % CACHE_LINE,
                0,
                "{target}: `{name}` starts at {address:#x}, off a {CACHE_LINE}-byte line"
            );
        }
    }
}

#[test]
fn every_function_of_a_release_build_starts_on_a_cache_line() {
    assert_functions_start_on_cache_lines(&CraneliftOptions {
        opt_level: OptLevel::Speed,
        runtime_checks: false,
        ..CraneliftOptions::default()
    });
}

#[test]
fn every_function_of_a_debug_build_starts_on_a_cache_line() {
    assert_functions_start_on_cache_lines(&CraneliftOptions::default());
}
