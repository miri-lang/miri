// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use miri::error::diagnostic::{DiagnosticBuilder, Severity};

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
