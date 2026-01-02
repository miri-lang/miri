// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_conditional_expression_basic() {
    let source = "
    let x = 10 if true else 20
    ";
    check_success(source);
}

#[test]
fn test_conditional_expression_condition_not_boolean() {
    let source = "
    let x = 10 if 1 else 20
    ";
    check_error(source, "Conditional condition must be a boolean");
}

#[test]
fn test_conditional_expression_branch_mismatch() {
    let source = "
    let x = 10 if true else 'hello'
    ";
    check_error(source, "Conditional branches must have the same type");
}

#[test]
fn test_conditional_expression_no_else_void() {
    let source = "
fn foo()
    return

let x = foo() if true
";
    check_success(source);
}

#[test]
fn test_conditional_expression_no_else_non_void() {
    let source = "
    let x = 10 if true
    ";
    check_error(
        source,
        "Conditional expression without else branch must return Void",
    );
}

#[test]
fn test_nested_conditional() {
    check_expr_type("1 if true else (2 if false else 3)", type_int());
}

#[test]
fn test_complex_condition() {
    check_expr_type("1 if (true and false) or (1 < 2) else 2", type_int());
}

#[test]
fn test_void_branches() {
    check_success(
        "
fn foo()
    return
fn bar()
    return
foo() if true else bar()
",
    );
}

#[test]
fn test_numeric_mismatch() {
    check_error(
        "1.0 if true else 1",
        "Conditional branches must have the same type",
    );
}

#[test]
fn test_nullable_mismatch() {
    // Currently fails because no common supertype inference
    check_error(
        "1 if true else None",
        "Conditional branches must have the same type",
    );
}

#[test]
fn test_inheritance_compatibility() {
    // This test verifies that we can use a subtype in the else branch
    // when the then branch is the supertype.
    // Note: This relies on are_compatible checking is_subtype(rhs, lhs).
    let source = "
struct A: x int
struct B: x int
type B extends A

let a = A(1)
let b = B(1)

let x = a if true else b
";
    check_vars_type(source, vec![("x", type_custom("A", None))]);
}

#[test]
fn test_inheritance_incompatibility() {
    // This test verifies that we cannot use a supertype in the else branch
    // when the then branch is the subtype (because the result type is fixed to then branch).
    let source = "
struct A: x int
struct B: x int
type B extends A

let a = A(1)
let b = B(1)

let x = b if true else a
";
    check_error(source, "Conditional branches must have the same type");
}
