// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_set_literal() {
    check_success("{ 1, 2, 3 }");
}

#[test]
fn test_set_literal_mixed_error() {
    check_error("{ 1, \"a\" }", "Set elements must have the same type");
}

#[test]
fn test_set_type_explicit() {
    check_success("var x {int} = {1, 2}");
}

#[test]
fn test_set_type_generic() {
    check_success("var x set<int> = {1, 2}");
}

#[test]
fn test_set_nullable_element_error() {
    check_error("var x {int?} = {None}", "Set elements cannot be nullable");
}

#[test]
fn test_set_nullable_element_generic_error() {
    check_error(
        "var x set<int?> = {None}",
        "Set elements cannot be nullable",
    );
}

#[test]
fn test_set_literal_with_none_error() {
    check_error("{None}", "Set elements cannot be nullable");
}

#[test]
fn test_set_literal_mixed_none_error() {
    check_error("{1, None}", "Set elements must have the same type");
}

#[test]
fn test_set_assignment() {
    check_success(
        "
var x = {1, 2}
x = {3, 4}
        ",
    );
}

#[test]
fn test_set_assignment_mismatch() {
    check_error(
        "
var x = {1, 2}
x = {\"a\"}
        ",
        "Type mismatch",
    );
}

#[test]
fn test_nested_sets() {
    check_success("var x {{int}} = {{1}, {2}}");
}

#[test]
fn test_set_membership() {
    check_success(
        "
let s = {1, 2, 3}
if 1 in s:
    let x = 1
        ",
    );
}

#[test]
fn test_set_membership_mismatch() {
    check_error(
        "
let s = {1, 2, 3}
if \"a\" in s:
    let x = 1
        ",
        "Type mismatch",
    );
}
