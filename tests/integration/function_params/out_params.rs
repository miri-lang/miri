// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_out_param_increment() {
    // Canonical `out` usage: callee increments the caller's variable.
    assert_type_checks(
        r#"
fn inc(x out int)
    x = x + 1

var n = 5
inc(n)
"#,
    );
}

#[test]
fn test_out_param_string_mutation() {
    // Callee appends to a string via an `out` parameter.
    assert_type_checks(
        r#"
use system.string

fn append_excl(s out String)
    s = f"{s}!"

var msg = "hello"
append_excl(msg)
"#,
    );
}

#[test]
fn test_multiple_out_params_swap() {
    // Callee swaps two caller variables via `out` parameters.
    assert_type_checks(
        r#"
fn swap(a out int, b out int)
    let tmp = a
    a = b
    b = tmp

var x = 1
var y = 2
swap(x, y)
"#,
    );
}
