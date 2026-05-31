// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::codegen::llvm::{LlvmBackend, LlvmOptions};
use miri::error::CodegenError;

use miri::codegen::backend::Backend;

#[test]
fn name_reports_llvm() {
    assert_eq!(LlvmBackend.name(), "llvm");
}

#[test]
fn compile_returns_not_supported_error() {
    let result = LlvmBackend.compile(&[], &LlvmOptions::default());
    let err = result.expect_err("LLVM backend stub must reject compile()");
    assert!(
        matches!(err, CodegenError::NotSupported { ref backend } if backend == "LLVM"),
        "expected CodegenError::NotSupported {{ backend: \"LLVM\" }}, got {err:?}",
    );
}

#[test]
fn display_marks_unimplemented_status() {
    assert_eq!(
        format!("{}", LlvmBackend),
        "LlvmBackend (not yet implemented)"
    );
}

#[test]
fn options_default_opt_level_is_zero() {
    assert_eq!(LlvmOptions::default().opt_level, 0);
}
