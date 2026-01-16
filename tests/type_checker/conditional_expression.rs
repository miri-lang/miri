// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_conditional_expression_basic() {
    let source = "
    let x = 10 if true else 20
    ";
    type_checker_test(source);
}

#[test]
fn test_conditional_expression_condition_not_boolean() {
    let source = "
    let x = 10 if 1 else 20
    ";
    type_checker_error_test(source, "Conditional condition must be a boolean");
}

#[test]
fn test_conditional_expression_branch_mismatch() {
    let source = "
    let x = 10 if true else 'hello'
    ";
    type_checker_error_test(source, "Conditional branches must have the same type");
}

#[test]
fn test_conditional_expression_no_else_void() {
    let source = "
fn foo()
    return

let x = foo() if true
";
    type_checker_test(source);
}

#[test]
fn test_conditional_expression_no_else_non_void() {
    let source = "
    let x = 10 if true
    ";
    type_checker_error_test(
        source,
        "Conditional expression without else branch must return Void",
    );
}

#[test]
fn test_nested_conditional() {
    type_checker_expr_type_test("1 if true else (2 if false else 3)", type_int());
}

#[test]
fn test_complex_condition() {
    type_checker_expr_type_test("1 if (true and false) or (1 < 2) else 2", type_int());
}

#[test]
fn test_void_branches() {
    type_checker_test(
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
    type_checker_error_test(
        "1.0 if true else 1",
        "Conditional branches must have the same type",
    );
}

#[test]
fn test_nullable_mismatch() {
    // Currently fails because no common supertype inference
    type_checker_error_test(
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
class A
    var x int

class B extends A
    var y int

let a = A()
let b = B()

let result = a if true else b
";
    type_checker_vars_type_test(source, vec![("result", type_custom("A", None))]);
}

#[test]
fn test_inheritance_incompatibility() {
    // This test verifies that we cannot use a supertype in the else branch
    // when the then branch is the subtype (because the result type is fixed to then branch).
    let source = "
class A
    var x int

class B extends A
    var y int

let a = A()
let b = B()

let result = b if true else a
";
    type_checker_error_test(source, "Conditional branches must have the same type");
}
