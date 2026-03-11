// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_array_creation() {
    assert_runs("let a = [1, 2, 3]");
}

#[test]
fn test_array_single_element() {
    assert_runs("let a = [42]");
}

#[test]
fn test_array_strings() {
    assert_runs(r#"let a = ["hello", "world"]"#);
}

#[test]
fn test_array_booleans() {
    assert_runs("let a = [true, false, true]");
}
