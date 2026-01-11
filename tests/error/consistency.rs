// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Tests for error infrastructure consistency.
//!
//! These tests verify that all error types properly implement the Reportable trait
//! and produce consistent, well-formatted diagnostics.

use miri::error::diagnostic::{Diagnostic, DiagnosticBuilder, Reportable, Severity};
use miri::error::format::format_diagnostic_full;
use miri::error::syntax::{Span, SyntaxErrorKind};
use miri::error::{
    CodegenError, InterpreterError, LoweringError, RuntimeError, SyntaxError, TypeError,
};

/// Helper to check that a diagnostic contains expected fields.
fn check_diagnostic(
    diag: &Diagnostic,
    expected_severity: Severity,
    has_code: bool,
    has_span: bool,
) {
    assert_eq!(diag.severity, expected_severity);
    assert!(
        !diag.title.is_empty(),
        "Diagnostic title should not be empty"
    );
    assert!(
        !diag.message.is_empty(),
        "Diagnostic message should not be empty"
    );
    assert_eq!(
        diag.code.is_some(),
        has_code,
        "Diagnostic code presence mismatch"
    );
    assert_eq!(
        diag.span.is_some(),
        has_span,
        "Diagnostic span presence mismatch"
    );
}

// ===== SyntaxError Tests =====

#[test]
fn test_syntax_error_reportable() {
    let error = SyntaxError::new(SyntaxErrorKind::InvalidToken, 0..5);
    let diag = error.to_diagnostic();

    check_diagnostic(&diag, Severity::Error, true, true);
    assert!(
        diag.code.unwrap().starts_with("E"),
        "Syntax error code should start with E"
    );
}

#[test]
fn test_syntax_error_all_variants_have_codes() {
    let variants: Vec<(SyntaxErrorKind, Span)> = vec![
        (SyntaxErrorKind::InvalidToken, 0..1),
        (SyntaxErrorKind::UnclosedMultilineComment, 0..5),
        (SyntaxErrorKind::IndentationMismatch, 0..1),
        (SyntaxErrorKind::UnclosedStringLiteral, 0..5),
        (
            SyntaxErrorKind::UnexpectedToken {
                expected: "foo".to_string(),
                found: "bar".to_string(),
            },
            0..3,
        ),
        (SyntaxErrorKind::UnexpectedEOF, 0..0),
        (SyntaxErrorKind::InvalidAssignmentTarget, 0..3),
        (SyntaxErrorKind::IntegerLiteralOverflow, 0..10),
    ];

    for (kind, span) in variants {
        let error = SyntaxError::new(kind.clone(), span);
        let diag = error.to_diagnostic();

        assert!(
            diag.code.is_some(),
            "SyntaxErrorKind::{:?} should have an error code",
            kind
        );
        assert!(
            !diag.title.is_empty(),
            "SyntaxErrorKind::{:?} should have a title",
            kind
        );
    }
}

// ===== TypeError Tests =====

#[test]
fn test_type_error_reportable() {
    let error = TypeError::new("Type mismatch".to_string(), 0..10);
    let diag = error.to_diagnostic();

    check_diagnostic(&diag, Severity::Error, false, true); // TypeError uses dynamic messages, no code
}

#[test]
fn test_type_error_with_help() {
    let error = TypeError::new("Unknown type 'intt'".to_string(), 0..4)
        .with_help("Did you mean 'int'?".to_string());
    let diag = error.to_diagnostic();

    assert!(diag.help.is_some());
    assert!(diag.help.unwrap().contains("Did you mean"));
}

// ===== LoweringError Tests =====

#[test]
fn test_lowering_error_reportable() {
    let error = LoweringError::new("unsupported expression", 0..5);
    let diag = error.to_diagnostic();

    check_diagnostic(&diag, Severity::Error, false, true);
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

// ===== RuntimeError Tests =====

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

// ===== CodegenError Tests =====

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

// ===== InterpreterError Tests =====

#[test]
fn test_interpreter_error_reportable() {
    let error = InterpreterError::UndefinedFunction("foo".to_string());
    let diag = error.to_diagnostic();

    check_diagnostic(&diag, Severity::Error, true, false);
}

#[test]
fn test_interpreter_error_variants() {
    let errors: Vec<InterpreterError> = vec![
        InterpreterError::UndefinedFunction("test".to_string()),
        InterpreterError::TypeMismatch {
            expected: "int".to_string(),
            got: "str".to_string(),
            context: "assignment".to_string(),
        },
        InterpreterError::DivisionByZero,
        InterpreterError::RemainderByZero,
        InterpreterError::Overflow,
        InterpreterError::StackOverflow,
        InterpreterError::NotImplemented("feature".to_string()),
        InterpreterError::Internal("error".to_string()),
    ];

    for error in errors {
        let diag = error.to_diagnostic();
        assert!(diag.code.is_some(), "InterpreterError should have code");
        assert!(!diag.title.is_empty(), "InterpreterError should have title");
    }
}

// ===== DiagnosticBuilder Tests =====

#[test]
fn test_diagnostic_builder() {
    let diag = DiagnosticBuilder::error("Test Error")
        .code("E9999")
        .message("This is a test error message")
        .span(10..20)
        .help("Try doing something different")
        .add_note("Additional context here")
        .build();

    assert_eq!(diag.severity, Severity::Error);
    assert_eq!(diag.code, Some("E9999"));
    assert_eq!(diag.title, "Test Error");
    assert_eq!(diag.message, "This is a test error message");
    assert_eq!(diag.span, Some(10..20));
    assert_eq!(diag.help, Some("Try doing something different".to_string()));
    assert_eq!(diag.notes.len(), 1);
}

#[test]
fn test_diagnostic_builder_warning() {
    let diag = DiagnosticBuilder::warning("Test Warning")
        .message("This is a warning")
        .build();

    assert_eq!(diag.severity, Severity::Warning);
}

// ===== Formatting Tests =====

#[test]
fn test_format_diagnostic_full_with_span() {
    let source = "let x = 42";
    let diag = DiagnosticBuilder::error("Test Error")
        .message("Something went wrong")
        .span(4..5)
        .build();

    let output = format_diagnostic_full(source, &diag);

    assert!(output.contains("error"), "Output should contain 'error'");
    assert!(
        output.contains("Test Error"),
        "Output should contain the title"
    );
}

#[test]
fn test_format_diagnostic_full_without_span() {
    let source = "";
    let diag = DiagnosticBuilder::error("No Span Error")
        .message("Error without source location")
        .build();

    let output = format_diagnostic_full(source, &diag);

    // Should not panic and should produce output
    assert!(output.contains("error"));
    assert!(output.contains("No Span Error"));
}

#[test]
fn test_format_diagnostic_full_warning() {
    let source = "let y = --x";
    let diag = DiagnosticBuilder::warning("Double Negation")
        .message("Double negation detected")
        .span(8..11)
        .build();

    let output = format_diagnostic_full(source, &diag);

    assert!(
        output.contains("warning"),
        "Output should contain 'warning'"
    );
}

// ===== Severity Tests =====

#[test]
fn test_severity_display() {
    assert_eq!(format!("{}", Severity::Error), "error");
    assert_eq!(format!("{}", Severity::Warning), "warning");
    assert_eq!(format!("{}", Severity::Note), "note");
}
