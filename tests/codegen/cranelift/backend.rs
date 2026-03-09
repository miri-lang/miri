// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::codegen::cranelift::CraneliftBackend;
use miri::codegen::cranelift::{CraneliftOptions, OptLevel};
use miri::codegen::Backend;

// ── Construction ───────────────────────────────────────────────────────

#[test]
fn test_cranelift_backend_new() {
    let backend = CraneliftBackend::new();
    assert!(backend.is_ok(), "Failed to create Cranelift backend");

    let backend = backend.unwrap();
    assert_eq!(backend.name(), "cranelift");
}

#[test]
fn test_cranelift_backend_for_host_target() {
    let target = target_lexicon::Triple::host();
    let backend = CraneliftBackend::for_target(target);
    assert!(backend.is_ok());
}

// ── Pointer type ───────────────────────────────────────────────────────

#[test]
fn test_pointer_type_matches_host() {
    let backend = CraneliftBackend::new().unwrap();
    let ptr_ty = backend.pointer_type();
    // On 64-bit hosts, pointer type should be I64
    if cfg!(target_pointer_width = "64") {
        assert_eq!(ptr_ty, cranelift_codegen::ir::types::I64);
    } else if cfg!(target_pointer_width = "32") {
        assert_eq!(ptr_ty, cranelift_codegen::ir::types::I32);
    }
}

// ── Target triple ──────────────────────────────────────────────────────

#[test]
fn test_target_returns_valid_triple() {
    let backend = CraneliftBackend::new().unwrap();
    let target = backend.target();
    // Should match the host triple (modulo macOS version fixup)
    let host = target_lexicon::Triple::host();
    assert_eq!(target.architecture, host.architecture);
}

// ── Setters ────────────────────────────────────────────────────────────

#[test]
fn test_set_type_definitions() {
    let mut backend = CraneliftBackend::new().unwrap();
    let defs = std::collections::HashMap::new();
    backend.set_type_definitions(defs);
    // No panic = success. Type defs are used internally during compile().
}

#[test]
fn test_set_runtime_imports() {
    let mut backend = CraneliftBackend::new().unwrap();
    backend.set_runtime_imports(vec![]);
    // No panic = success.
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
    assert_eq!(artifact.format, miri::codegen::ArtifactFormat::ObjectFile);
}

#[test]
fn test_compile_with_all_opt_levels() {
    let backend = CraneliftBackend::new().unwrap();

    for opt in [OptLevel::None, OptLevel::Speed, OptLevel::SpeedAndSize] {
        let options = CraneliftOptions {
            opt_level: opt,
            pic: true,
        };
        let result = backend.compile(&[], &options);
        assert!(
            result.is_ok(),
            "Compile with opt_level {:?} should succeed, got: {:?}",
            opt,
            result.err()
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
    let result = backend.compile(&[], &options);
    assert!(
        result.is_ok(),
        "Compile with PIC disabled should succeed, got: {:?}",
        result.err()
    );
}
