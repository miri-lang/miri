// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_set_creation() {
    assert_runs("let s = {1, 2, 3}");
}

#[test]
fn test_set_creation_strings() {
    assert_runs("let s = {'a', 'b', 'c'}");
}

#[test]
fn test_set_creation_single() {
    assert_runs("let s = {42}");
}

#[test]
fn test_set_explicit_constructor_with_literal_int() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

let s = Set({1, 2, 3})
println(f"{s.length()}")
"#,
        "3",
    );
}

#[test]
fn test_set_explicit_constructor_with_literal_string() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

let s = Set({"alpha", "beta", "gamma"})
let has_beta = s.contains("beta")
println(f"{s.length()}")
println(f"{has_beta}")
"#,
        "3\ntrue",
    );
}

#[test]
fn test_set_explicit_constructor_empty_typed() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

var s = Set<int>()
s.add(7)
println(f"{s.length()}")
"#,
        "1",
    );
}
