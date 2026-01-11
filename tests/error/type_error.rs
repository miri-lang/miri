// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::check_diagnostic;
use miri::error::diagnostic::{Reportable, Severity};
use miri::error::TypeError;

#[test]
fn test_type_error_reportable() {
    let error = TypeError::custom("Type mismatch".to_string(), 0..10, None);
    let diag = error.to_diagnostic();

    check_diagnostic(&diag, Severity::Error, true, true); // TypeError uses dynamic messages, no code
}

#[test]
fn test_type_error_with_help() {
    let error = TypeError::custom(
        "Unknown type 'intt'".to_string(),
        0..4,
        Some("Did you mean 'int'?".to_string()),
    );
    let diag = error.to_diagnostic();

    assert!(diag.help.is_some());
    assert!(diag.help.unwrap().contains("Did you mean"));
}
