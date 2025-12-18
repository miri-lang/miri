// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast_factory::*;
use miri::syntax_error::SyntaxErrorKind;

#[test]
fn test_includes_statement_single() {
    parser_test(
        "includes Enumerable",
        vec![includes(vec![identifier("Enumerable")])],
    );
}

#[test]
fn test_includes_statement_multiple() {
    parser_test(
        "includes Enumerable, Utils",
        vec![includes(vec![
            identifier("Enumerable"),
            identifier("Utils"),
        ])],
    );
}

#[test]
fn test_error_includes_missing_identifier() {
    parser_error_test(
        "includes",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "end of file".to_string(),
        },
    );
}
