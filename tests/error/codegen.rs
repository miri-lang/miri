// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::check_diagnostic;
use miri::error::diagnostic::{Reportable, Severity};
use miri::error::CodegenError;

#[test]
fn test_codegen_error_reportable() {
    let error = CodegenError::target_isa("unknown target");
    let diag = error.to_diagnostic();

    check_diagnostic(&diag, Severity::Error, true, false);
}

#[test]
fn test_codegen_error_constructors() {
    let errors = vec![
        CodegenError::target_isa("test"),
        CodegenError::module("test"),
        CodegenError::declare_function("main", "error"),
        CodegenError::define_function("main", "error"),
        CodegenError::translation("main", "error"),
        CodegenError::emit("error"),
        CodegenError::not_supported("LLVM"),
    ];

    for error in errors {
        let diag = error.to_diagnostic();
        assert!(diag.code.is_some());
        assert!(!diag.title.is_empty());
    }
}
