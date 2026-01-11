// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::check_diagnostic;
use miri::error::diagnostic::{Reportable, Severity};
use miri::error::LoweringError;

#[test]
fn test_lowering_error_reportable() {
    let error = LoweringError::custom("unsupported expression".to_string(), 0..5, None);
    let diag = error.to_diagnostic();

    check_diagnostic(&diag, Severity::Error, true, true);
}

#[test]
fn test_lowering_error_factory_methods() {
    let errors = vec![
        LoweringError::unsupported_expression("match", 0..5),
        LoweringError::unsupported_statement("async", 0..5),
        LoweringError::undefined_variable("x", 0..1),
        LoweringError::break_outside_loop(0..5),
        LoweringError::continue_outside_loop(0..8),
    ];

    for error in errors {
        let diag = error.to_diagnostic();
        assert!(!diag.message.is_empty());
        assert!(diag.span.is_some());
    }
}
