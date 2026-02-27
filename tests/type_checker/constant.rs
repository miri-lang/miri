// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::type_checker::utils::{
    type_checker_const_type_test, type_checker_error_test, type_checker_test,
};
use miri::ast::factory::make_type;
use miri::ast::types::TypeKind;

#[test]
fn const_integer() {
    type_checker_test("const x = 10");
}

#[test]
fn const_typed_integer() {
    type_checker_test("const x i32 = 10");
}

#[test]
fn const_string() {
    type_checker_test("const x = \"hello\"");
}

#[test]
fn const_boolean() {
    type_checker_test("const flag = true");
}

#[test]
fn const_assignment_is_error() {
    type_checker_error_test(
        "
const x = 10
x = 20
        ",
        "Cannot assign to constant",
    );
}

#[test]
fn const_type_mismatch() {
    type_checker_error_test("const x i32 = \"hello\"", "Type mismatch");
}

#[test]
fn const_inferred_type() {
    type_checker_const_type_test("const x = 10", vec![("x", make_type(TypeKind::Int))]);
}

#[test]
fn const_explicit_type() {
    type_checker_const_type_test("const x i32 = 10", vec![("x", make_type(TypeKind::I32))]);
}

#[test]
fn const_shadowing_disallowed_same_scope() {
    type_checker_error_test(
        "
const x = 10
const x = 20
        ",
        "Cannot shadow existing variable/constant 'x' with a constant.",
    );
}

#[test]
fn const_shadowing_disallowed_outer_scope() {
    type_checker_error_test(
        "
const x = 10
if true:
    const x = 20
        ",
        "Cannot shadow existing variable/constant 'x' with a constant.",
    );
}

#[test]
fn const_shadowing_variable_disallowed() {
    type_checker_error_test(
        "
let x = 10
const x = 20
        ",
        "Cannot shadow existing variable/constant 'x' with a constant.",
    );
}

#[test]
fn variable_shadowing_const_disallowed() {
    type_checker_error_test(
        "
const x = 10
let x = 20
        ",
        "Cannot shadow constant 'x'.",
    );
}

#[test]
fn variable_shadowing_const_disallowed_nested() {
    type_checker_error_test(
        "
const x = 10
if true:
    let x = 20
        ",
        "Cannot shadow constant 'x'.",
    );
}
