// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

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

// ===== Class Member Visibility =====

use crate::type_checker::utils::{check_error, check_success};

#[test]
fn test_private_field_access_from_same_class() {
    let code = "
class Person
    private var age int
    fn getAge() int
        self.age
    ";
    check_success(code);
}

#[test]
fn test_private_method_access_from_same_class() {
    let code = "
class Calculator
    private fn secret() int
        42
    fn reveal() int
        self.secret()
    ";
    check_success(code);
}

// Note: Protected field/method access from subclass tests are skipped
// because inheritance lookup for parent fields/methods is not
// yet implemented in the type checker (self.parentField doesn't work).

#[test]
fn test_public_field_access_from_other_class() {
    let code = "
class Person
    public var name String

class Greeter
    fn greet(p Person) String
        p.name
    ";
    check_success(code);
}

#[test]
fn test_private_field_access_from_other_class_error() {
    let code = "
class Person
    private var secret int

class Snooper
    fn spy(p Person) int
        p.secret
    ";
    check_error(code, "is Private and cannot be accessed");
}

#[test]
fn test_protected_field_access_from_non_subclass_error() {
    let code = "
class Person
    protected var ssn String

class Hacker
    fn steal(p Person) String
        p.ssn
    ";
    check_error(code, "is Protected and cannot be accessed");
}

#[test]
fn test_private_method_access_from_other_class_error() {
    let code = "
class SecureVault
    private fn unlock() int
        42

class Thief
    fn tryUnlock(v SecureVault) int
        v.unlock()
    ";
    check_error(code, "is Private and cannot be accessed");
}
