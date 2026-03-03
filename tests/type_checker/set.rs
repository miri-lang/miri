// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_set_literal() {
    type_checker_test("{ 1, 2, 3 }");
}

#[test]
fn test_set_literal_mixed_error() {
    type_checker_error_test("{ 1, \"a\" }", "Set elements must have the same type");
}

#[test]
fn test_set_type_explicit() {
    type_checker_test("var x {int} = {1, 2}");
}

#[test]
fn test_set_type_generic() {
    type_checker_test("var x set<int> = {1, 2}");
}

#[test]
fn test_set_nullable_element_error() {
    type_checker_error_test("var x {int?} = {None}", "Set elements cannot be optional");
}

#[test]
fn test_set_nullable_element_generic_error() {
    type_checker_error_test(
        "var x set<int?> = {None}",
        "Set elements cannot be optional",
    );
}

#[test]
fn test_set_literal_with_none_error() {
    type_checker_error_test("{None}", "Set elements cannot be optional");
}

#[test]
fn test_set_literal_mixed_none_error() {
    type_checker_error_test("{1, None}", "Set elements must have the same type");
}

#[test]
fn test_set_assignment() {
    type_checker_test(
        "
var x = {1, 2}
x = {3, 4}
        ",
    );
}

#[test]
fn test_set_assignment_mismatch() {
    type_checker_error_test(
        "
var x = {1, 2}
x = {\"a\"}
        ",
        "Type mismatch",
    );
}

#[test]
fn test_nested_sets() {
    type_checker_test("var x {{int}} = {{1}, {2}}");
}

#[test]
fn test_set_membership() {
    type_checker_test(
        "
let s = {1, 2, 3}
if 1 in s:
    let x = 1
        ",
    );
}

#[test]
fn test_set_membership_mismatch() {
    type_checker_error_test(
        "
let s = {1, 2, 3}
if \"a\" in s:
    let x = 1
        ",
        "Type mismatch",
    );
}
