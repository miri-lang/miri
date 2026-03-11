// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_string_eq() {
    assert_runs_with_output(
        r#"
use system.io

let a = "hello"
let b = "hello"
let c = "world"
let r1 = if a == b: 1 else: 0
let r2 = if a == c: 1 else: 0
println(f"{r1}")
println(f"{r2}")
"#,
        "1",
    );
}

#[test]
fn test_string_ne() {
    assert_runs_with_output(
        r#"
use system.io

let a = "foo"
let b = "bar"
let c = "foo"
let r1 = if a != b: 1 else: 0
let r2 = if a != c: 1 else: 0
println(f"{r1}")
println(f"{r2}")
"#,
        "1",
    );
}
