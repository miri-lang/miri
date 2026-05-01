// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

// ─────────────────────────────────────────────────────
// let variable passed to out param → error
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_let_is_error() {
    assert_compiler_error(
        r#"
fn inc(x out int)
    x = x + 1

let n = 5
inc(n)
"#,
        "expected mutable variable for 'out' parameter",
    );
}

// ─────────────────────────────────────────────────────
// var variable passed to out param → ok
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_var_ok() {
    assert_type_checks(
        r#"
fn inc(x out int)
    x = x + 1

var n = 5
inc(n)
"#,
    );
}

// ─────────────────────────────────────────────────────
// non-variable expression passed to out param → error
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_literal_is_error() {
    assert_compiler_error(
        r#"
fn inc(x out int)
    x = x + 1

inc(5)
"#,
        "expected mutable variable for 'out' parameter",
    );
}

// ─────────────────────────────────────────────────────
// same variable passed twice as out → error
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_duplicate_var_is_error() {
    assert_compiler_error(
        r#"
fn swap(a out int, b out int)
    let tmp = a
    a = b
    b = tmp

var x = 1
swap(x, x)
"#,
        "same variable passed twice as 'out'",
    );
}

// ─────────────────────────────────────────────────────
// mixed: first param regular, second out — duplicate only
// for the out params, not the regular param
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_duplicate_only_out_params_tracked() {
    assert_type_checks(
        r#"
fn update(a int, b out int)
    b = a + b

var x = 1
update(x, x)
"#,
    );
}

// ─────────────────────────────────────────────────────
// type mismatch for out param → error
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_type_mismatch_is_error() {
    assert_compiler_error(
        r#"
fn inc(x out int)
    x = x + 1

var n = 5.0
inc(n)
"#,
        "Type mismatch",
    );
}

// ─────────────────────────────────────────────────────
// managed-type out param: variable must not be consumed
// after the call — the callee wrote a new value to it
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_managed_type_not_consumed_after_call() {
    assert_type_checks(
        r#"
use system.string

fn append_excl(s out String)
    s = f"{s}!"

var msg = "hello"
append_excl(msg)
let result = msg
"#,
    );
}

// ─────────────────────────────────────────────────────
// resource-type out param: variable must not be consumed
// after the call, and must be usable before the call
// even if previously consumed (callee writes, not reads)
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_resource_type_not_consumed_after_call() {
    assert_type_checks(
        r#"
struct Conn
    handle int
    fn drop(self)
        return

fn reset(c out Conn)
    c = Conn(handle: 99)

var c = Conn(handle: 1)
reset(c)
let x = c
"#,
    );
}
