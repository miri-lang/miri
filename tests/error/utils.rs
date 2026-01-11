// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use miri::error::diagnostic::{Diagnostic, Severity};

/// Helper to check that a diagnostic contains expected fields.
pub fn check_diagnostic(
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
