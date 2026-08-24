// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::diagnostics::DiagnosticCode;
use miri::error::syntax::Span;

use super::utils::check_diagnostic;
use miri::error::diagnostic::{Reportable, Severity};
use miri::error::TypeError;

#[test]
fn test_type_error_reportable() {
    let error = TypeError::coded(
        DiagnosticCode::TypTypeMismatch,
        "Type mismatch".to_string(),
        Span::new(0, 10),
        None,
    );
    let diag = error.to_diagnostic();

    check_diagnostic(&diag, Severity::Error, true, true);
    assert_eq!(diag.code, Some(DiagnosticCode::TypTypeMismatch.as_str()));
}

#[test]
fn test_type_error_with_help() {
    let error = TypeError::coded(
        DiagnosticCode::TypTypeNotFound,
        "Unknown type 'intt'".to_string(),
        Span::new(0, 4),
        Some("Did you mean 'int'?".to_string()),
    );
    let diag = error.to_diagnostic();

    assert!(diag.help.is_some());
    assert!(diag.help.unwrap().contains("Did you mean"));
}
