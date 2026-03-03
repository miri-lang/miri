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

#[test]
fn test_levenshtein_distance() {
    use miri::error::format::levenshtein_distance;
    assert_eq!(levenshtein_distance("", ""), 0);
    assert_eq!(levenshtein_distance("abc", "abc"), 0);
    assert_eq!(levenshtein_distance("abc", ""), 3);
    assert_eq!(levenshtein_distance("", "abc"), 3);
    assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
    assert_eq!(levenshtein_distance("flaw", "lawn"), 2);
    assert_eq!(levenshtein_distance("saturday", "sunday"), 3);
    assert_eq!(levenshtein_distance("rust", "rustacean"), 5);
}

#[test]
fn test_find_best_match_exact() {
    use miri::error::format::find_best_match;
    let candidates = vec!["apple", "banana", "cherry"];
    assert_eq!(find_best_match("apple", &candidates), Some("apple".to_string()));
}

#[test]
fn test_find_best_match_close() {
    use miri::error::format::find_best_match;
    let candidates = vec!["apple", "banana", "cherry"];
    assert_eq!(find_best_match("appl", &candidates), Some("apple".to_string()));
}

#[test]
fn test_find_best_match_threshold() {
    use miri::error::format::find_best_match;
    let candidates = vec!["apple", "banana", "cherry"];
    assert_eq!(find_best_match("abc", &candidates), None);
}

#[test]
fn test_find_best_match_empty_candidates() {
    use miri::error::format::find_best_match;
    let candidates: Vec<&str> = vec![];
    assert_eq!(find_best_match("apple", &candidates), None);
}

#[test]
fn test_find_best_match_best_of_multiple() {
    use miri::error::format::find_best_match;
    let candidates = vec!["apple", "apply", "app"];
    assert_eq!(find_best_match("appl", &candidates), Some("apple".to_string()));
}

#[test]
fn test_find_best_match_unicode() {
    use miri::error::format::find_best_match;
    let candidates = vec!["🦀rust", "🚀fast"];
    assert_eq!(find_best_match("🦀rust", &candidates), Some("🦀rust".to_string()));
    assert_eq!(find_best_match("🦀rus", &candidates), Some("🦀rust".to_string()));
}
