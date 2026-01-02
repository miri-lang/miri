// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_shadowing_same_scope_same_type() {
    let source = "
let x = 1
let x = 2
x
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_shadowing_same_scope_different_type() {
    let source = "
let x = 1
let x = \"string\"
x
    ";
    check_expr_type(source, type_string());
}

#[test]
fn test_shadowing_var_with_let() {
    let source = "
var x = 1
let x = \"string\"
x
    ";
    check_error(
        source,
        "Cannot shadow mutable variable 'x' with an immutable one in the same scope.",
    );
}

#[test]
fn test_shadowing_let_with_var() {
    let source = "
let x = 1
var x = \"string\"
x
    ";
    check_error(
        source,
        "Variable 'x' is already defined in this scope. 'var' cannot shadow existing variables.",
    );
}

#[test]
fn test_shadowing_nested_scope() {
    let source = "
let x = 1
if true:
    let x = \"string\"
    x
    ";
    // We can't easily check the type of the inner expression directly with check_expr_type
    // because it returns the type of the last statement in the block.
    // But we can verify it compiles.
    check_success(source);
}

#[test]
fn test_shadowing_nested_scope_restoration() {
    let source = "
let x = 1
if true:
    let x = \"string\"
x
    ";
    // Should be Int (outer x)
    check_expr_type(source, type_int());
}

#[test]
fn test_shadowing_parameter() {
    let source = "
fn foo(x int) string:
    let x = \"string\"
    return x

foo(1)
    ";
    check_expr_type(source, type_string());
}

#[test]
fn test_shadowing_loop_variable() {
    let source = "
for i in 1..10:
    let i = \"string\"
    ";
    check_success(source);
}

#[test]
fn test_shadowing_match_variable() {
    let source = "
let x = 1
match x
    y: y // This y shadows the outer x if we used x, but here we test binding
    ";
    check_success(source);
}

#[test]
fn test_shadowing_match_variable_shadow() {
    let source = "
let x = 1
match x
    x: x
    ";
    check_success(source);
}

#[test]
fn test_shadowing_global_with_local() {
    let source = "
let x = 1
fn foo() string:
    let x = \"string\"
    return x

foo()
    ";
    check_expr_type(source, type_string());
}
