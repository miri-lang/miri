// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ─── Type-checker error cases ─────────────────────────────────────────────

#[test]
fn test_out_param_immutable_var_rejected() {
    assert_compiler_error(
        r#"
fn inc(x out int)
    x = x + 1

let n = 5
inc(n)
"#,
        "immutable",
    );
}

#[test]
fn test_out_param_literal_rejected() {
    assert_compiler_error(
        r#"
fn inc(x out int)
    x = x + 1

inc(5)
"#,
        "non-variable expression",
    );
}

#[test]
fn test_out_param_same_var_twice_rejected() {
    assert_compiler_error(
        r#"
fn swap(a out int, b out int)
    let tmp = a
    a = b
    b = tmp

var x = 1
swap(x, x)
"#,
        "same variable passed twice",
    );
}

// ─── Codegen tests (AC 8.3) ───────────────────────────────────────────────

#[test]
fn test_out_param_int_writeback() {
    assert_runs_with_output(
        r#"
use system.io

fn inc(x out int)
    x = x + 1

fn main()
    var n = 5
    inc(n)
    println(f"{n}")
"#,
        "6",
    );
}

#[test]
fn test_out_param_list_push() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn append(list out [int])
    list.push(99)

fn main()
    var l = List([1, 2])
    append(l)
    println(f"{l[0]} {l[1]} {l[2]}")
"#,
        "1 2 99",
    );
}

#[test]
fn test_out_param_int_multiple() {
    assert_runs_with_output(
        r#"
use system.io

fn swap(a out int, b out int)
    let tmp = a
    a = b
    b = tmp

fn main()
    var x = 10
    var y = 20
    swap(x, y)
    println(f"{x} {y}")
"#,
        "20 10",
    );
}

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

#[test]
fn test_out_param_bool_writeback() {
    // Bool maps to i8 in Cranelift — exercises a distinct scalar path from i64.
    assert_runs_with_output(
        r#"
use system.io

fn toggle(flag out bool)
    flag = not flag

fn main()
    var f = false
    toggle(f)
    println(f"{f}")
"#,
        "true",
    );
}

#[test]
fn test_out_param_float_writeback() {
    // f32 out param — exercises the float stack-slot path in codegen.
    assert_runs_with_output(
        r#"
use system.io

fn double(x out f32)
    x = x * 2.0

fn main()
    var n = 3.5
    double(n)
    println(f"{n}")
"#,
        "7.0",
    );
}
