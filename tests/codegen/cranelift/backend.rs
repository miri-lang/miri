// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use cranelift_codegen::ir::types;
use cranelift_object::object::{File, Object, ObjectSymbol};
use miri::ast::literal::Literal;
use miri::ast::types::{Type, TypeKind};
use miri::ast::MemberVisibility;
use miri::codegen::cranelift::{CraneliftBackend, CraneliftOptions, OptLevel, RuntimeImport};
use miri::codegen::{ArtifactFormat, Backend};
use miri::error::syntax::Span;
use miri::mir::symbol::Symbol;
use miri::mir::type_facts::{StructDefinition, TypeDefinition, TypeFacts};
use miri::mir::{
    BasicBlockData, Body, Constant, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
};
use miri::type_checker::ModuleId;

use std::collections::HashMap;

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

// ── Construction & Target ──────────────────────────────────────────────

#[test]
fn test_cranelift_backend_new() {
    let backend = CraneliftBackend::new().expect("Failed to create Cranelift backend");
    assert_eq!(backend.name(), "cranelift");

    let host_triple = target_lexicon::Triple::host();
    assert_eq!(backend.target().architecture, host_triple.architecture);

    // Verify for_target with explicit host target triple matches
    let backend_explicit =
        CraneliftBackend::for_target(host_triple.clone()).expect("Failed for explicit target");
    assert_eq!(backend_explicit.name(), "cranelift");
    assert_eq!(
        backend_explicit.target().architecture,
        host_triple.architecture
    );
}

// ── Pointer type ───────────────────────────────────────────────────────

#[test]
fn test_pointer_type_matches_host() {
    let backend = CraneliftBackend::new().unwrap();
    let ptr_ty = backend.pointer_type();
    if cfg!(target_pointer_width = "64") {
        assert_eq!(ptr_ty, types::I64);
    } else if cfg!(target_pointer_width = "32") {
        assert_eq!(ptr_ty, types::I32);
    }
}

// ── Target triple ──────────────────────────────────────────────────────

#[test]
fn test_target_returns_valid_triple() {
    let backend = CraneliftBackend::new().unwrap();
    let target = backend.target();
    let host = target_lexicon::Triple::host();
    assert_eq!(target.architecture, host.architecture);
}

// ── Setters ────────────────────────────────────────────────────────────

#[test]
fn test_set_type_facts() {
    let mut backend = CraneliftBackend::new().unwrap();

    // Define a struct type facts definition and body using it
    let struct_def = StructDefinition {
        fields: vec![
            ("x".to_string(), ty(TypeKind::I64), MemberVisibility::Public),
            ("y".to_string(), ty(TypeKind::I64), MemberVisibility::Public),
        ],
        generics: None,
        module: String::new(),
    };
    let definitions = HashMap::from([("Point".to_string(), TypeDefinition::Struct(struct_def))]);

    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ty(TypeKind::Void), Span::default()));
    body.new_local(LocalDecl::new(
        ty(TypeKind::Custom("Point".to_string(), None)),
        Span::default(),
    ));
    let mut block = BasicBlockData::new(None);
    block.statements.push(Statement {
        kind: StatementKind::DecRef(Place::new(Local(1))),
        span: Span::default(),
    });
    block.terminator = Some(Terminator::new(TerminatorKind::Return, Span::default()));
    body.basic_blocks.push(block);

    let facts = TypeFacts::new(definitions, HashMap::new(), Default::default(), [&body]).unwrap();

    backend.set_type_facts(facts);

    let options = CraneliftOptions::default();
    let artifact = backend
        .compile(&[("test_point_drop", &body)], &options)
        .expect("Compilation with set type facts should succeed");

    assert_eq!(artifact.format, ArtifactFormat::ObjectFile);
    assert!(
        !artifact.bytes.is_empty(),
        "Artifact compiled with type facts must produce object bytes"
    );
}

#[test]
fn test_set_runtime_imports() {
    let mut backend = CraneliftBackend::new().unwrap();

    let custom_import_name = "test_custom_rt_import_fn";
    let import = RuntimeImport {
        name: custom_import_name.to_string(),
        param_types: vec![types::I64, types::I64],
        return_type: Some(types::I64),
    };

    backend.set_runtime_imports(vec![import]);

    // An object with no code has no sections, which a Mach-O reader refuses
    // to parse, so the module carries one empty body beside the import.
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ty(TypeKind::Void), Span::default()));
    let mut block = BasicBlockData::new(None);
    block.terminator = Some(Terminator::new(TerminatorKind::Return, Span::default()));
    body.basic_blocks.push(block);

    let options = CraneliftOptions::default();
    let artifact = backend
        .compile(&[("test_runtime_import_host", &body)], &options)
        .expect("Compilation with runtime imports should succeed");

    assert_eq!(artifact.format, ArtifactFormat::ObjectFile);
    assert!(!artifact.bytes.is_empty());

    // Parse emitted object file and verify symbol import presence
    let obj = File::parse(&*artifact.bytes).expect("Artifact bytes must be a valid object file");
    let symbol_exists = obj.symbols().any(|s| {
        if let Ok(name) = s.name() {
            name == custom_import_name || name == format!("_{}", custom_import_name)
        } else {
            false
        }
    });

    assert!(
        symbol_exists,
        "Object artifact must contain declared external runtime import symbol '{custom_import_name}'"
    );
}

// ── Display / Debug ────────────────────────────────────────────────────

#[test]
fn test_display_contains_target() {
    let backend = CraneliftBackend::new().unwrap();
    let display = format!("{}", backend);
    assert!(
        display.contains("CraneliftBackend"),
        "Display should contain 'CraneliftBackend', got: {}",
        display
    );
    assert!(
        display.contains("target="),
        "Display should contain 'target=', got: {}",
        display
    );
    assert!(
        display.contains(&backend.target().to_string()),
        "Display should contain the target triple string"
    );
}

#[test]
fn test_debug_contains_target() {
    let backend = CraneliftBackend::new().unwrap();
    let debug = format!("{:?}", backend);
    assert!(
        debug.contains("CraneliftBackend"),
        "Debug should contain 'CraneliftBackend', got: {}",
        debug
    );
    assert!(
        debug.contains(&backend.target().to_string()),
        "Debug should contain target triple in debug representation, got: {}",
        debug
    );
}

// ── Options defaults ───────────────────────────────────────────────────

#[test]
fn test_cranelift_options_default() {
    let opts = CraneliftOptions::default();
    assert_eq!(opts.opt_level, OptLevel::None);
    assert!(opts.pic, "PIC should be true by default");
}

#[test]
fn test_opt_level_default() {
    let level = OptLevel::default();
    assert_eq!(level, OptLevel::None);
}

// ── Compile with empty bodies ──────────────────────────────────────────

#[test]
fn test_compile_empty_bodies() {
    let backend = CraneliftBackend::new().unwrap();
    let options = CraneliftOptions::default();
    let result = backend.compile(&[], &options);
    assert!(
        result.is_ok(),
        "Compiling zero functions should succeed, got: {:?}",
        result.err()
    );

    let artifact = result.unwrap();
    assert!(
        !artifact.bytes.is_empty(),
        "Even empty compilation should produce an object file header"
    );
    assert_eq!(artifact.format, ArtifactFormat::ObjectFile);
}

#[test]
fn test_compile_with_all_opt_levels() {
    let backend = CraneliftBackend::new().unwrap();

    for opt in [OptLevel::None, OptLevel::Speed, OptLevel::SpeedAndSize] {
        let options = CraneliftOptions {
            opt_level: opt,
            pic: true,
        };
        let artifact = backend
            .compile(&[], &options)
            .unwrap_or_else(|e| panic!("Compile with opt_level {opt:?} should succeed: {e}"));

        assert_eq!(artifact.format, ArtifactFormat::ObjectFile);
        assert!(
            !artifact.bytes.is_empty(),
            "Compilation with opt_level {opt:?} must produce non-empty artifact"
        );
    }
}

#[test]
fn test_compile_with_pic_disabled() {
    let backend = CraneliftBackend::new().unwrap();
    let options = CraneliftOptions {
        opt_level: OptLevel::None,
        pic: false,
    };
    let artifact = backend
        .compile(&[], &options)
        .expect("Compile with PIC disabled should succeed");

    assert_eq!(artifact.format, ArtifactFormat::ObjectFile);
    assert!(
        !artifact.bytes.is_empty(),
        "Compilation with PIC disabled must produce non-empty artifact"
    );
}

// ── Callees ────────────────────────────────────────────────────────────

/// A body of locals `_0: void` and `_1: int` whose one block runs
/// `statements` and ends in `terminator`, then returns.
fn body_ending_in(statements: Vec<Statement>, terminator: TerminatorKind) -> Body {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ty(TypeKind::Void), Span::default()));
    body.new_local(LocalDecl::new(ty(TypeKind::Int), Span::default()));
    let mut first = BasicBlockData::new(None);
    first.statements = statements;
    first.terminator = Some(Terminator {
        kind: terminator,
        span: Span::default(),
    });
    let mut last = BasicBlockData::new(None);
    last.terminator = Some(Terminator {
        kind: TerminatorKind::Return,
        span: Span::default(),
    });
    body.basic_blocks.push(first);
    body.basic_blocks.push(last);
    body
}

fn compile_error(body: &Body) -> String {
    let backend = CraneliftBackend::new().expect("host backend");
    match backend.compile(&[("caller", body)], &CraneliftOptions::default()) {
        Ok(_) => panic!("the body must be refused"),
        Err(error) => error.to_string(),
    }
}

/// A call whose callee is a constant names no function; it is an internal
/// error, never an indirect call through whatever the constant reads as.
#[test]
fn a_call_through_a_constant_callee_is_an_internal_error() {
    let body = body_ending_in(
        Vec::new(),
        TerminatorKind::Call {
            func: Operand::Constant(Box::new(Constant {
                span: Span::default(),
                ty: ty(TypeKind::Identifier),
                literal: Literal::Identifier("miri.foo".to_string()),
            })),
            args: Vec::new(),
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(Local(1)),
            target: Some(miri::mir::BasicBlock(1)),
        },
    );
    let error = compile_error(&body);
    assert!(error.contains("not a function"), "{error}");
}

/// A function read as a value whose operand carries no function type has no
/// signature to take an address under; it is an internal error, never a null
/// function pointer.
#[test]
fn a_function_value_without_a_function_type_is_an_internal_error() {
    let read = Statement {
        kind: StatementKind::Assign(
            Place::new(Local(1)),
            Rvalue::Use(Operand::function(
                Symbol::declared_function(&ModuleId::Program, "foo"),
                Span::default(),
            )),
        ),
        span: Span::default(),
    };
    let body = body_ending_in(
        vec![read],
        TerminatorKind::Goto {
            target: miri::mir::BasicBlock(1),
        },
    );
    let error = compile_error(&body);
    assert!(error.contains("carries no function type"), "{error}");
}
