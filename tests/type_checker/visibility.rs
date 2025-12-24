// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::type_checker::utils::{check_multi_module_error, check_multi_module_success};

#[test]
fn test_visibility_same_module() {
    check_multi_module_success(vec![("A", "private let x = 1"), ("A", "x")]);
}

#[test]
fn test_visibility_different_module() {
    check_multi_module_error(
        vec![("A", "private let x = 1"), ("B", "x")],
        "Variable 'x' is not visible",
    );
}

#[test]
fn test_visibility_public_different_module() {
    check_multi_module_success(vec![("A", "public let x = 1"), ("B", "x")]);
}

#[test]
fn test_function_visibility() {
    // Public function - accessible
    check_multi_module_success(vec![("A", "public fn foo()\n    1"), ("B", "foo()")]);

    // Private function - not accessible
    check_multi_module_error(
        vec![("A", "private fn foo()\n    1"), ("B", "foo()")],
        "Variable 'foo' is not visible",
    );
}

#[test]
fn test_struct_visibility() {
    // Public struct - accessible
    check_multi_module_success(vec![
        ("A", "public struct Point: x int, y int"),
        ("B", "let p = Point(x: 1, y: 2)"),
    ]);

    // Private struct - not accessible
    check_multi_module_error(
        vec![
            ("A", "private struct Point: x int, y int"),
            ("B", "let p = Point(x: 1, y: 2)"),
        ],
        "Variable 'Point' is not visible",
    );
}

#[test]
fn test_enum_visibility() {
    // Public enum - accessible
    check_multi_module_success(vec![
        ("A", "public enum Color: Red, Green"),
        ("B", "let c = Color.Red"),
    ]);

    // Private enum - not accessible
    check_multi_module_error(
        vec![
            ("A", "private enum Color: Red, Green"),
            ("B", "let c = Color.Red"),
        ],
        "Variable 'Color' is not visible",
    );
}
