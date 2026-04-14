// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::check_diagnostic;
use miri::error::diagnostic::{Reportable, Severity};
use miri::error::syntax::Span;
use miri::error::{LoweringError, LoweringErrorKind};

#[test]
fn test_lowering_error_reportable() {
    let error = LoweringError::custom("unsupported expression".to_string(), Span::new(0, 5), None);
    let diag = error.to_diagnostic();

    check_diagnostic(&diag, Severity::Error, true, true);
}

#[test]
fn test_lowering_error_unsupported_operator() {
    let span = Span::new(10, 12);
    let error = LoweringError::unsupported_operator("@", span);

    // Check error kind
    match &error.kind {
        LoweringErrorKind::UnsupportedOperator { op } => assert_eq!(op, "@"),
        _ => panic!("Expected UnsupportedOperator error kind"),
    }

    let diag = error.to_diagnostic();
    assert_eq!(diag.severity, Severity::Error);
    assert_eq!(diag.code, Some("E0207"));
    assert_eq!(diag.title, "Unsupported Operator");
    assert!(diag.message.contains("@"));
    assert_eq!(diag.span, Some(span));
}

#[test]
fn test_lowering_error_factory_methods() {
    let errors = vec![
        LoweringError::unsupported_expression("match", Span::new(0, 5)),
        LoweringError::unsupported_statement("async", Span::new(0, 5)),
        LoweringError::undefined_variable("x", Span::new(0, 1)),
        LoweringError::type_not_found(42, Span::new(0, 5)),
        LoweringError::break_outside_loop(Span::new(0, 5)),
        LoweringError::continue_outside_loop(Span::new(0, 8)),
        LoweringError::unsupported_lhs("constant", Span::new(0, 5)),
        LoweringError::unsupported_operator(">>>", Span::new(0, 3)),
        LoweringError::unsupported_range_type(Span::new(0, 5)),
        LoweringError::invalid_gpu_launch_args(2, 3, Span::new(0, 10)),
        LoweringError::unsupported_type("f128", Span::new(0, 4)),
        LoweringError::missing_struct_field("x", "Point", Span::new(0, 5)),
    ];

    for error in errors {
        let diag = error.to_diagnostic();
        assert!(!diag.message.is_empty(), "Message should not be empty for {:?}", error.kind);
        assert!(diag.span.is_some(), "Span should be present for {:?}", error.kind);
    }
}
