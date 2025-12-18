// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_nullable_assignment() {
    check_success("var x int? = 5");
}

#[test]
fn test_none_assignment_to_nullable() {
    check_success("var x int? = None");
}

#[test]
fn test_none_assignment_to_non_nullable_error() {
    check_error("var x int = None", "Type mismatch");
}

#[test]
fn test_nullable_immutable_warning() {
    check_warning(
        "let x int? = 5",
        "Variable 'x' is immutable but declared as nullable",
    );
}
