// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::check_diagnostic;
use miri::error::diagnostic::{Reportable, Severity};
use miri::error::InterpreterError;

#[test]
fn test_interpreter_error_reportable() {
    let error = InterpreterError::undefined_function("foo");
    let diag = error.to_diagnostic();

    check_diagnostic(&diag, Severity::Error, true, false);
}

#[test]
fn test_interpreter_error_variants() {
    let errors: Vec<InterpreterError> = vec![
        InterpreterError::undefined_function("test"),
        InterpreterError::type_mismatch("int", "str", "assignment"),
        InterpreterError::division_by_zero(),
        InterpreterError::remainder_by_zero(),
        InterpreterError::overflow(),
        InterpreterError::stack_overflow(),
        InterpreterError::not_implemented("feature"),
        InterpreterError::internal("error"),
    ];

    for error in errors {
        let diag = error.to_diagnostic();
        assert!(diag.code.is_some(), "InterpreterError should have code");
        assert!(!diag.title.is_empty(), "InterpreterError should have title");
    }
}
