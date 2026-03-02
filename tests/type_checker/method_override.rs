// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::type_checker::utils::{type_checker_error_test, type_checker_test};

// ===== Valid Method Overrides =====

#[test]
fn test_override_method_same_signature() {
    // Overriding with exact same signature should work
    let code = "
class Animal
    fn speak() int
        1

class Dog extends Animal
    fn speak() int
        2
    ";
    type_checker_test(code);
}

#[test]
fn test_override_method_with_parameters() {
    // Overriding method with parameters
    let code = "
class Calculator
    fn add(a int, b int) int
        a + b

class AdvancedCalculator extends Calculator
    fn add(a int, b int) int
        a + b + 1
    ";
    type_checker_test(code);
}

// ===== Invalid Method Overrides =====

#[test]
fn test_override_method_wrong_return_type_error() {
    // Changing return type should be an error
    let code = "
class Animal
    fn speak() int
        1

class Dog extends Animal
    fn speak() String
        \"bark\"
    ";
    type_checker_error_test(code, "incompatible return type");
}

#[test]
fn test_override_method_wrong_param_count_error() {
    // Changing parameter count should be an error
    let code = "
class Animal
    fn speak(volume int) int
        volume

class Dog extends Animal
    fn speak() int
        1
    ";
    type_checker_error_test(code, "incompatible parameter");
}

#[test]
fn test_override_method_wrong_param_type_error() {
    // Changing parameter type should be an error
    let code = "
class Calculator
    fn add(a int, b int) int
        a + b

class WrongCalculator extends Calculator
    fn add(a String, b int) int
        1
    ";
    type_checker_error_test(code, "incompatible parameter");
}

// ===== Multi-level Override =====

#[test]
fn test_override_multi_level() {
    // Override through multiple inheritance levels
    let code = "
class Base
    fn value() int
        1

class Middle extends Base
    fn value() int
        2

class Derived extends Middle
    fn value() int
        3
    ";
    type_checker_test(code);
}

#[test]
fn test_override_skipping_level() {
    // Override a method from grandparent (not defined in immediate parent)
    let code = "
class Base
    fn value() int
        1

class Middle extends Base
    fn other() int
        2

class Derived extends Middle
    fn value() int
        3
    ";
    type_checker_test(code);
}
