// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_runs, assert_runs_with_output};

#[test]
fn test_tuple_creation() {
    assert_runs("let t = (1, 2, 3)");
}

#[test]
fn test_tuple_single_element() {
    assert_runs("let t = (42,)");
}

#[test]
fn test_tuple_mixed_types() {
    assert_runs(r#"let t = (1, "hello", true)"#);
}

#[test]
fn test_tuple_access() {
    assert_runs_with_output(
        r#"
use system.io

let t = (10, 20, 30)
print(f"{t.0 + t.1}")
    "#,
        "30",
    );
}
