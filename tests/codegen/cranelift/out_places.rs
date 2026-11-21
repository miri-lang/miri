// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A field or an element taken for an `out` argument, as the backend passes
//! it.
//!
//! The bodies are hand-built: a struct `W { k int, inner Inner }` whose
//! `inner` field, an `Inner { x int, name String }` held as a counted block,
//! is handed to a callee that replaces it whole, and an element of an array
//! of them handed the same way. Each program exits with the `x` the caller
//! reads back out of the place after the call: 42 only when the callee's
//! write landed in the place itself.

use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::type_arguments::{TypeArgKind, TypeArgument};
use miri::ast::types::{Type, TypeKind};
use miri::ast::MemberVisibility;
use miri::codegen::cranelift::{CraneliftBackend, CraneliftOptions};
use miri::codegen::Backend;
use miri::error::syntax::Span;
use miri::mir::place::PlaceElem;
use miri::mir::rc::RcClassifier;
use miri::mir::symbol::Symbol;
use miri::mir::type_facts::TypeFacts;
use miri::mir::{
    AggregateKind, ArgMode, BasicBlock, BasicBlockData, Body, Constant, ExecutionModel, Local,
    LocalDecl, Operand, Place, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};
use miri::type_checker::context::{StructDefinition, TypeDefinition};
use miri::type_checker::ModuleId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

fn span() -> Span {
    Span::default()
}

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

fn inner() -> TypeKind {
    TypeKind::Custom("Inner".to_string(), None)
}

fn wrapper() -> TypeKind {
    TypeKind::Custom("W".to_string(), None)
}

fn struct_of(fields: Vec<(&str, TypeKind)>) -> TypeDefinition {
    let fields = fields
        .into_iter()
        .map(|(name, kind)| (name.to_string(), ty(kind), MemberVisibility::Public))
        .collect();
    TypeDefinition::Struct(StructDefinition {
        fields,
        generics: None,
        module: String::new(),
    })
}

/// `struct Inner { x int, name String }` and `struct W { k int, inner Inner }`.
fn definitions() -> HashMap<String, TypeDefinition> {
    HashMap::from([
        (
            "Inner".to_string(),
            struct_of(vec![("x", TypeKind::Int), ("name", TypeKind::String)]),
        ),
        (
            "W".to_string(),
            struct_of(vec![("k", TypeKind::Int), ("inner", inner())]),
        ),
    ])
}

fn place(local: usize) -> Place {
    Place::new(Local(local))
}

fn projected(local: usize, projection: Vec<PlaceElem>) -> Place {
    Place {
        local: Local(local),
        projection,
    }
}

fn int(value: i64) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: span(),
        ty: ty(TypeKind::Int),
        literal: Literal::Integer(IntegerLiteral::I64(value)),
    }))
}

fn string(value: &str) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: span(),
        ty: ty(TypeKind::String),
        literal: Literal::String(value.to_string()),
    }))
}

fn assign(dest: Place, rvalue: Rvalue) -> Statement {
    Statement {
        kind: StatementKind::Assign(dest, rvalue),
        span: span(),
    }
}

fn block(statements: Vec<Statement>, kind: TerminatorKind) -> BasicBlockData {
    let mut data = BasicBlockData::new(None);
    data.statements = statements;
    data.terminator = Some(Terminator::new(kind, span()));
    data
}

fn body_of(kinds: &[TypeKind], arg_count: usize, blocks: Vec<BasicBlockData>) -> Body {
    let mut body = Body::new(arg_count, span(), ExecutionModel::Cpu);
    for kind in kinds {
        body.new_local(LocalDecl::new(ty(kind.clone()), span()));
    }
    body.basic_blocks.extend(blocks);
    body.rc_classifier = RcClassifier::new(Rc::new(definitions()));
    body
}

fn put() -> Symbol {
    Symbol::function(&ModuleId::Program, "put", &[])
}

/// `fn put(p out Inner, q Inner)`: `p = q`.
fn put_body() -> Body {
    let mut body = body_of(
        &[TypeKind::Void, inner(), inner()],
        2,
        vec![block(
            vec![assign(place(1), Rvalue::Use(Operand::Copy(place(2))))],
            TerminatorKind::Return,
        )],
    );
    body.signature.out_params = vec![true, false];
    body
}

fn build(kind: TypeKind, fields: Vec<Operand>) -> Rvalue {
    Rvalue::Aggregate(AggregateKind::Struct(ty(kind)), fields)
}

/// `main`: `_2 = Inner(7, "a")`, `_1 = W(1, _2)`, `_3 = Inner(42, "b")`, then
/// `put(out <taken>, _3)`, and exits with `_1.inner.x`.
fn main_body(taken: Place) -> Body {
    let setup = vec![
        assign(place(2), build(inner(), vec![int(7), string("a")])),
        assign(
            place(1),
            build(wrapper(), vec![int(1), Operand::Copy(place(2))]),
        ),
        assign(place(3), build(inner(), vec![int(42), string("b")])),
    ];
    let call = TerminatorKind::Call {
        func: Operand::function(put(), span()),
        args: vec![Operand::Copy(taken), Operand::Copy(place(3))],
        arg_modes: vec![ArgMode::Out, ArgMode::Borrowed],
        arg_handles: Vec::new(),
        flags: miri::mir::CallFlags::default(),
        destination: place(4),
        target: Some(BasicBlock(1)),
    };
    let read_back = projected(1, vec![PlaceElem::Field(1), PlaceElem::Field(0)]);
    let exit = vec![assign(place(0), Rvalue::Use(Operand::Copy(read_back)))];
    body_of(
        &[TypeKind::Int, wrapper(), inner(), inner(), TypeKind::Void],
        0,
        vec![block(setup, call), block(exit, TerminatorKind::Return)],
    )
}

/// The field `inner` of `_1`.
fn inner_field() -> Place {
    projected(1, vec![PlaceElem::Field(1)])
}

fn backend_for(bodies: &[&Body]) -> CraneliftBackend {
    let mut backend = CraneliftBackend::new().expect("host backend");
    let facts = TypeFacts::new(
        definitions(),
        HashMap::new(),
        Default::default(),
        bodies.iter().copied(),
    )
    .expect("type facts");
    backend.set_type_facts(facts);
    backend
}

/// Compile `main` beside `put`, returning the object or the error.
fn compile(main: &Body) -> Result<Vec<u8>, String> {
    let callee = put_body();
    let named = [
        (put().link_name(), &callee),
        (Symbol::entry().link_name(), main),
    ];
    let bodies: Vec<&Body> = named.iter().map(|(_, body)| *body).collect();
    let named: Vec<(&str, &Body)> = (named.iter())
        .map(|(name, body)| (name.as_str(), *body))
        .collect();
    backend_for(&bodies)
        .compile(&named, &CraneliftOptions::default())
        .map(|artifact| artifact.bytes)
        .map_err(|error| error.to_string())
}

fn runtime_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runtime/core/target/release")
}

/// Link `object` against the core runtime and run it, returning its exit
/// status.
fn run(object: &[u8]) -> i32 {
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("miri-out-places-{}-{run}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch directory");
    let object_path = dir.join("program.o");
    let exe_path = dir.join("program");
    std::fs::write(&object_path, object).expect("object written");
    let status = Command::new("cc")
        .arg(&object_path)
        .arg("-o")
        .arg(&exe_path)
        .arg(format!("-L{}", runtime_dir().display()))
        .arg("-lmiri_runtime_core")
        .arg("-lm")
        .status()
        .expect("the linker runs");
    assert!(status.success(), "linking failed: {status}");
    let exit = Command::new(&exe_path).status().expect("the program runs");
    let _ = std::fs::remove_dir_all(&dir);
    exit.code().expect("the program exits")
}

fn array_of_inner() -> TypeKind {
    let argument = TypeArgument::new(0, TypeArgKind::Type(Box::new(ty(inner())), false), span());
    let size = miri::ast::type_arguments::value_argument(2);
    TypeKind::Array(Box::new(argument), Box::new(size))
}

/// `main`: `_3 = [Inner(7, "a"), Inner(8, "c"); 2]`, `_4 = 1`,
/// `_5 = Inner(42, "b")`, then `put(out _3[_4], _5)`, and exits with
/// `_3[_4].x`.
fn element_main() -> Body {
    let element = projected(3, vec![PlaceElem::index(Local(4))]);
    let setup = vec![
        assign(place(1), build(inner(), vec![int(7), string("a")])),
        assign(place(2), build(inner(), vec![int(8), string("c")])),
        assign(
            place(3),
            Rvalue::Aggregate(
                AggregateKind::Array,
                vec![Operand::Copy(place(1)), Operand::Copy(place(2))],
            ),
        ),
        assign(place(4), Rvalue::Use(int(1))),
        assign(place(5), build(inner(), vec![int(42), string("b")])),
    ];
    let call = TerminatorKind::Call {
        func: Operand::function(put(), span()),
        args: vec![Operand::Copy(element.clone()), Operand::Copy(place(5))],
        arg_modes: vec![ArgMode::Out, ArgMode::Borrowed],
        arg_handles: Vec::new(),
        flags: miri::mir::CallFlags::default(),
        destination: place(6),
        target: Some(BasicBlock(1)),
    };
    let mut read_back = element;
    read_back.projection.push(PlaceElem::Field(0));
    let exit = vec![assign(place(0), Rvalue::Use(Operand::Copy(read_back)))];
    body_of(
        &[
            TypeKind::Int,
            inner(),
            inner(),
            array_of_inner(),
            TypeKind::Int,
            inner(),
            TypeKind::Void,
        ],
        0,
        vec![block(setup, call), block(exit, TerminatorKind::Return)],
    )
}

#[test]
fn a_field_taken_out_is_written_in_place_by_the_callee() {
    let object = match compile(&main_body(inner_field())) {
        Ok(object) => object,
        Err(error) => panic!("the program compiles: {error}"),
    };
    assert_eq!(run(&object), 42);
}

#[test]
fn an_element_taken_out_is_written_in_place_by_the_callee() {
    let object = match compile(&element_main()) {
        Ok(object) => object,
        Err(error) => panic!("the program compiles: {error}"),
    };
    assert_eq!(run(&object), 42);
}

#[test]
fn a_place_taken_out_through_a_dereference_is_refused() {
    let through = projected(1, vec![PlaceElem::Field(1), PlaceElem::Deref]);
    let Err(error) = compile(&main_body(through)) else {
        panic!("the call must be refused");
    };
    assert!(error.contains("`out` argument"), "{error}");
}

fn array_of_wrappers() -> TypeKind {
    let argument = TypeArgument::new(0, TypeArgKind::Type(Box::new(ty(wrapper())), false), span());
    let size = miri::ast::type_arguments::value_argument(2);
    TypeKind::Array(Box::new(argument), Box::new(size))
}

/// `main`: `_5 = [W(1, Inner(7, "a")), W(2, Inner(8, "c")); 2]`, `_6 = 1`,
/// `_7 = Inner(42, "b")`, then `put(out _5[_6].inner, _7)`, and exits with
/// `_5[_6].inner.x`.
fn nested_main() -> Body {
    let taken = projected(5, vec![PlaceElem::index(Local(6)), PlaceElem::Field(1)]);
    let setup = vec![
        assign(place(1), build(inner(), vec![int(7), string("a")])),
        assign(
            place(2),
            build(wrapper(), vec![int(1), Operand::Copy(place(1))]),
        ),
        assign(place(3), build(inner(), vec![int(8), string("c")])),
        assign(
            place(4),
            build(wrapper(), vec![int(2), Operand::Copy(place(3))]),
        ),
        assign(
            place(5),
            Rvalue::Aggregate(
                AggregateKind::Array,
                vec![Operand::Copy(place(2)), Operand::Copy(place(4))],
            ),
        ),
        assign(place(6), Rvalue::Use(int(1))),
        assign(place(7), build(inner(), vec![int(42), string("b")])),
    ];
    let call = TerminatorKind::Call {
        func: Operand::function(put(), span()),
        args: vec![Operand::Copy(taken.clone()), Operand::Copy(place(7))],
        arg_modes: vec![ArgMode::Out, ArgMode::Borrowed],
        arg_handles: Vec::new(),
        flags: miri::mir::CallFlags::default(),
        destination: place(8),
        target: Some(BasicBlock(1)),
    };
    let mut read_back = taken;
    read_back.projection.push(PlaceElem::Field(0));
    let exit = vec![assign(place(0), Rvalue::Use(Operand::Copy(read_back)))];
    body_of(
        &[
            TypeKind::Int,
            inner(),
            wrapper(),
            inner(),
            wrapper(),
            array_of_wrappers(),
            TypeKind::Int,
            inner(),
            TypeKind::Void,
        ],
        0,
        vec![block(setup, call), block(exit, TerminatorKind::Return)],
    )
}

#[test]
fn a_field_of_an_element_taken_out_is_written_in_place_by_the_callee() {
    let object = match compile(&nested_main()) {
        Ok(object) => object,
        Err(error) => panic!("the program compiles: {error}"),
    };
    assert_eq!(run(&object), 42);
}

#[test]
fn a_scalar_field_taken_by_its_address_is_refused() {
    let scalar = projected(1, vec![PlaceElem::Field(0)]);
    let Err(error) = compile(&main_body(scalar)) else {
        panic!("the call must be refused");
    };
    assert!(
        error.contains("a slot of its type is copied in and out"),
        "{error}"
    );
}
