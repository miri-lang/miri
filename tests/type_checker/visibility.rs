// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::type_checker::utils::{
    type_checker_multi_module_error_test, type_checker_multi_module_test,
};

#[test]
fn test_visibility_same_module() {
    type_checker_multi_module_test(vec![("A", "private let x = 1"), ("A", "x")]);
}

#[test]
fn test_visibility_different_module() {
    type_checker_multi_module_error_test(
        vec![("A", "private let x = 1"), ("B", "x")],
        "Variable 'x' is not visible",
    );
}

#[test]
fn test_visibility_public_different_module() {
    type_checker_multi_module_test(vec![("A", "public let x = 1"), ("B", "x")]);
}

#[test]
fn test_function_visibility() {
    // Public function - accessible
    type_checker_multi_module_test(vec![("A", "public fn foo()\n    1"), ("B", "foo()")]);

    // Private function - not accessible
    type_checker_multi_module_error_test(
        vec![("A", "private fn foo()\n    1"), ("B", "foo()")],
        "Variable 'foo' is not visible",
    );
}

#[test]
fn test_struct_visibility() {
    // Public struct - accessible
    type_checker_multi_module_test(vec![
        ("A", "public struct Point: x int, y int"),
        ("B", "let p = Point(x: 1, y: 2)"),
    ]);

    // Private struct - not accessible
    type_checker_multi_module_error_test(
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
    type_checker_multi_module_test(vec![
        ("A", "public enum Color: Red, Green"),
        ("B", "let c = Color.Red"),
    ]);

    // Private enum - not accessible
    type_checker_multi_module_error_test(
        vec![
            ("A", "private enum Color: Red, Green"),
            ("B", "let c = Color.Red"),
        ],
        "Variable 'Color' is not visible",
    );
}

// ===== Class Member Visibility =====

use crate::type_checker::utils::{type_checker_error_test, type_checker_test};

#[test]
fn test_private_field_access_from_same_class() {
    let code = "
class Person
    private var age int
    fn getAge() int
        self.age
    ";
    type_checker_test(code);
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
    type_checker_test(code);
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
    type_checker_test(code);
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
    type_checker_error_test(code, "is Private and cannot be accessed");
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
    type_checker_error_test(code, "is Protected and cannot be accessed");
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
    type_checker_error_test(code, "is Private and cannot be accessed");
}
