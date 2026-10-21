// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for closure trampoline emission.
//!
//! Every closure a runtime call takes is followed by the address of the
//! trampoline the runtime calls it through. The backend emits each trampoline
//! once, however many calls name it, and only after checking that the
//! signature it calls the closure under is the one the closure's type gives.

use cranelift_codegen::ir::types;
use cranelift_object::object::{File, Object, ObjectSymbol};
use miri::ast::common::Parameter;
use miri::ast::factory::type_expr_non_null;
use miri::ast::types::{FunctionTypeData, Type, TypeKind};
use miri::codegen::cranelift::{CraneliftBackend, CraneliftOptions, RuntimeImport};
use miri::codegen::Backend;
use miri::error::syntax::Span;
use miri::mir::closure_abi::pass_closures_with_trampolines;
use miri::mir::symbol::{Symbol, TrampolineArgument, TrampolineShape, TrampolineWord};
use miri::mir::{
    ArgMode, BasicBlock, BasicBlockData, Body, ExecutionModel, Local, LocalDecl, Operand, Place,
    Terminator, TerminatorKind,
};

const SORT_BY: &str = "miri_rt_list_sort_by";

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn comparator_of(element: TypeKind) -> TypeKind {
    let param = || Parameter {
        name: String::new(),
        name_span: Span::default(),
        typ: Box::new(type_expr_non_null(ty(element.clone()))),
        guard: None,
        default_value: None,
        is_out: false,
        residency: None,
    };
    TypeKind::Function(Box::new(FunctionTypeData {
        generics: None,
        params: vec![param(), param()],
        return_type: Some(Box::new(type_expr_non_null(ty(TypeKind::Int)))),
    }))
}

/// A body sorting a list of `element` by a comparator over `element`, with the
/// closure ABI spelled.
fn sorting_by(element: TypeKind) -> Body {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ty(TypeKind::Void), Span::default()));
    let list = TypeKind::List(Box::new(type_expr_non_null(ty(element.clone()))));
    let list = body.new_local(LocalDecl::new(ty(list), Span::default()));
    let closure = body.new_local(LocalDecl::new(ty(comparator_of(element)), Span::default()));
    let call = TerminatorKind::Call {
        func: Operand::runtime(SORT_BY, Span::default()),
        args: vec![
            Operand::Copy(Place::new(list)),
            Operand::Copy(Place::new(closure)),
        ],
        arg_modes: vec![ArgMode::Borrowed; 2],
        arg_handles: Vec::new(),
        destination: Place::new(Local(0)),
        target: Some(BasicBlock(1)),
    };
    body.basic_blocks
        .push(BasicBlockData::new(Some(Terminator::new(
            call,
            Span::default(),
        ))));
    body.basic_blocks
        .push(BasicBlockData::new(Some(Terminator::new(
            TerminatorKind::Return,
            Span::default(),
        ))));
    pass_closures_with_trampolines(&mut body).expect("the call keeps the contract");
    body
}

fn backend() -> CraneliftBackend {
    let mut backend = CraneliftBackend::new().expect("host backend");
    backend.set_runtime_imports(vec![RuntimeImport {
        name: SORT_BY.to_string(),
        param_types: vec![types::I64; 3],
        return_type: None,
    }]);
    backend
}

/// How many symbols of the object compiled from `bodies` are named `name`.
fn symbols_named(bodies: &[(&str, &Body)], name: &str) -> usize {
    let artifact = backend()
        .compile(bodies, &CraneliftOptions::default())
        .unwrap_or_else(|e| panic!("compiling failed: {e:?}"));
    let object = File::parse(&*artifact.bytes).expect("an object file");
    object
        .symbols()
        .filter(|symbol| symbol.is_definition())
        .filter_map(|symbol| symbol.name().ok())
        .filter(|symbol| symbol.trim_start_matches('_') == name.trim_start_matches('_'))
        .count()
}

fn word_comparator() -> String {
    Symbol::closure_trampoline(TrampolineShape {
        arguments: vec![TrampolineArgument::Word(TrampolineWord::Pointer); 2],
        result: Some(TrampolineWord::Pointer),
    })
    .link_name()
}

#[test]
fn a_trampoline_two_calls_name_is_emitted_once() {
    let (first, second) = (sorting_by(TypeKind::Int), sorting_by(TypeKind::Int));
    let bodies = [("first_sort", &first), ("second_sort", &second)];
    assert_eq!(symbols_named(&bodies, &word_comparator()), 1);
}

#[test]
fn closures_of_different_shapes_get_a_trampoline_each() {
    let (words, narrow) = (sorting_by(TypeKind::Int), sorting_by(TypeKind::I32));
    let bodies = [("word_sort", &words), ("narrow_sort", &narrow)];
    let narrow_comparator = Symbol::closure_trampoline(TrampolineShape {
        arguments: vec![TrampolineArgument::Word(TrampolineWord::I32); 2],
        result: Some(TrampolineWord::Pointer),
    })
    .link_name();
    assert_eq!(symbols_named(&bodies, &word_comparator()), 1);
    assert_eq!(symbols_named(&bodies, &narrow_comparator), 1);
}

#[test]
fn a_trampoline_that_would_call_a_closure_under_another_signature_is_refused() {
    let mut body = sorting_by(TypeKind::Int);
    let Some(TerminatorKind::Call { args, .. }) = body.basic_blocks[0]
        .terminator
        .as_mut()
        .map(|t| &mut t.kind)
    else {
        panic!("the body ends without its call");
    };
    let Some(Operand::Function(trampoline)) = args.last_mut() else {
        panic!("the closure has no trampoline after it");
    };
    trampoline.symbol = Symbol::closure_trampoline(TrampolineShape {
        arguments: vec![TrampolineArgument::Word(TrampolineWord::I128); 2],
        result: Some(TrampolineWord::Pointer),
    });
    let error = backend()
        .compile(&[("mismatched_sort", &body)], &CraneliftOptions::default())
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(error.contains("would call it as"), "{error}");
}
