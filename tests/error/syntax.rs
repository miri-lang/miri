// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::check_diagnostic;
use miri::error::diagnostic::{Reportable, Severity};
use miri::error::syntax::{find_line_info, Span, SyntaxErrorKind};
use miri::error::SyntaxError;

#[test]
fn test_syntax_error_reportable() {
    let error = SyntaxError::new(SyntaxErrorKind::InvalidToken, Span::new(0, 5));
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
        (SyntaxErrorKind::InvalidToken, Span::new(0, 1)),
        (SyntaxErrorKind::UnclosedMultilineComment, Span::new(0, 5)),
        (SyntaxErrorKind::IndentationMismatch, Span::new(0, 1)),
        (SyntaxErrorKind::UnclosedStringLiteral, Span::new(0, 5)),
        (
            SyntaxErrorKind::UnexpectedToken {
                expected: "foo".to_string(),
                found: "bar".to_string(),
            },
            Span::new(0, 3),
        ),
        (SyntaxErrorKind::UnexpectedEOF, Span::new(0, 0)),
        (SyntaxErrorKind::InvalidAssignmentTarget, Span::new(0, 3)),
        (SyntaxErrorKind::IntegerLiteralOverflow, Span::new(0, 10)),
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

#[test]
fn test_find_line_info() {
    // Empty string
    assert_eq!(find_line_info("", 0), (1, 1, ""));
    assert_eq!(find_line_info("", 10), (1, 1, "")); // Out of bounds behaves gracefully

    // Single line
    let single = "hello world";
    assert_eq!(find_line_info(single, 0), (1, 1, "hello world"));
    assert_eq!(find_line_info(single, 6), (1, 7, "hello world"));
    assert_eq!(find_line_info(single, single.len()), (1, 12, "hello world")); // EOF

    // Multiple lines
    let multi = "first\nsecond\nthird";
    assert_eq!(find_line_info(multi, 0), (1, 1, "first"));
    assert_eq!(find_line_info(multi, 5), (1, 6, "first"));
    assert_eq!(find_line_info(multi, 6), (2, 1, "second"));
    assert_eq!(find_line_info(multi, 10), (2, 5, "second"));
    assert_eq!(find_line_info(multi, 13), (3, 1, "third"));
    assert_eq!(find_line_info(multi, multi.len()), (3, 6, "third")); // EOF

    // Out of bounds pos
    assert_eq!(find_line_info(multi, 100), (3, 6, "third")); // Limits to the end of the last line string

    // Multi-byte characters
    let unicode = "hëllo\nwörld";
    // 'ë' is 2 bytes, so 'h' is index 0, 'ë' is index 1..2, 'l' is index 3
    assert_eq!(find_line_info(unicode, 0), (1, 1, "hëllo")); // 'h'
    assert_eq!(find_line_info(unicode, 1), (1, 2, "hëllo")); // 'ë'
    assert_eq!(find_line_info(unicode, 3), (1, 3, "hëllo")); // 'l' (1st)
    assert_eq!(find_line_info(unicode, 6), (1, 6, "hëllo")); // '\n' (col_num includes preceding chars + 1)

    // Line 2: "wörld" starts at index 7. 'ö' is 2 bytes.
    assert_eq!(find_line_info(unicode, 7), (2, 1, "wörld")); // 'w'
    assert_eq!(find_line_info(unicode, 8), (2, 2, "wörld")); // 'ö'
    assert_eq!(find_line_info(unicode, 10), (2, 3, "wörld")); // 'r'
}
