use miri::error::syntax::Span;
// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::error::diagnostic::DiagnosticBuilder;
use miri::error::format::format_diagnostic_full;

#[test]
fn test_format_diagnostic_full_with_span() {
    let source = "let x = 42";
    let diag = DiagnosticBuilder::error("Test Error")
        .message("Something went wrong")
        .span(Span::new(4, 5))
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
        .span(Span::new(8, 11))
        .build();

    let output = format_diagnostic_full(source, &diag);

    assert!(
        output.contains("warning"),
        "Output should contain 'warning'"
    );
}
