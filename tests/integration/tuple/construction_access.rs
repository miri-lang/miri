// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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

#[test]
fn test_tuple_nested() {
    assert_runs_with_output(
        r#"
use system.io

let t = ((1, 2), (3, 4))
let sum = t.0.0 + t.0.1 + t.1.0 + t.1.1
println(f"{sum}")
"#,
        "10",
    );
}
