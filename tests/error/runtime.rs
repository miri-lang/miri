// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::check_diagnostic;
use miri::error::diagnostic::{Reportable, Severity};
use miri::error::RuntimeError;

#[test]
fn test_runtime_error_reportable() {
    let error = RuntimeError::DivisionByZero;
    let diag = error.to_diagnostic();

    check_diagnostic(&diag, Severity::Error, true, false); // No span for runtime errors
    assert_eq!(diag.code.unwrap(), "E0400");
}

#[test]
fn test_runtime_error_all_variants() {
    let variants = vec![RuntimeError::DivisionByZero, RuntimeError::RemainderByZero];

    for error in variants {
        let diag = error.to_diagnostic();
        assert!(diag.code.is_some());
        assert!(!diag.title.is_empty());
    }
}
