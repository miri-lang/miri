// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::check_diagnostic;
use miri::error::diagnostic::{Reportable, Severity};
use miri::error::syntax::{Span, SyntaxErrorKind};
use miri::error::SyntaxError;

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
