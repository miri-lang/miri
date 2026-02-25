use miri::error::syntax::Span;
// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::check_diagnostic;
use miri::error::diagnostic::{Reportable, Severity};
use miri::error::LoweringError;

#[test]
fn test_lowering_error_reportable() {
    let error = LoweringError::custom("unsupported expression".to_string(), Span::new(0, 5), None);
    let diag = error.to_diagnostic();

    check_diagnostic(&diag, Severity::Error, true, true);
}

#[test]
fn test_lowering_error_factory_methods() {
    let errors = vec![
        LoweringError::unsupported_expression("match", Span::new(0, 5)),
        LoweringError::unsupported_statement("async", Span::new(0, 5)),
        LoweringError::undefined_variable("x", Span::new(0, 1)),
        LoweringError::break_outside_loop(Span::new(0, 5)),
        LoweringError::continue_outside_loop(Span::new(0, 8)),
    ];

    for error in errors {
        let diag = error.to_diagnostic();
        assert!(!diag.message.is_empty());
        assert!(diag.span.is_some());
    }
}
