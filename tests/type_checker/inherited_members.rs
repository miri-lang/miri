// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::type_checker::utils::{type_checker_error_test, type_checker_test};

// ===== Inherited Field Access =====

#[test]
fn test_access_parent_public_field_via_self() {
    let code = "
class Animal
    public var name String

class Dog extends Animal
    fn getName() String
        self.name
    ";
    type_checker_test(code);
}

#[test]
fn test_access_parent_protected_field_via_self() {
    let code = "
class Animal
    protected var age int

class Dog extends Animal
    fn getAge() int
        self.age
    ";
    type_checker_test(code);
}

#[test]
fn test_access_parent_private_field_via_self_error() {
    let code = "
class Animal
    private var secret int

class Dog extends Animal
    fn getSecret() int
        self.secret
    ";
    type_checker_error_test(code, "Private and cannot be accessed");
}

// ===== Inherited Method Access =====

#[test]
fn test_access_parent_public_method_via_self() {
    let code = "
class Animal
    public fn speak() String
        \"sound\"

class Dog extends Animal
    fn bark() String
        self.speak()
    ";
    type_checker_test(code);
}

#[test]
fn test_access_parent_protected_method_via_self() {
    let code = "
class Animal
    protected fn getInfo() String
        \"info\"

class Dog extends Animal
    fn info() String
        self.getInfo()
    ";
    type_checker_test(code);
}

#[test]
fn test_access_parent_private_method_via_self_error() {
    let code = "
class Animal
    private fn secretMethod() String
        \"secret\"

class Dog extends Animal
    fn tryAccess() String
        self.secretMethod()
    ";
    type_checker_error_test(code, "Private and cannot be accessed");
}

// ===== Multi-level Inheritance =====

#[test]
fn test_multi_level_field_access() {
    let code = "
class Animal
    protected var name String

class Mammal extends Animal
    protected var legs int

class Dog extends Mammal
    fn describe() String
        self.name
    ";
    type_checker_test(code);
}

#[test]
fn test_multi_level_method_access() {
    let code = "
class Animal
    protected fn baseMethod() int
        1

class Mammal extends Animal
    protected fn middleMethod() int
        2

class Dog extends Mammal
    fn callBoth() int
        self.baseMethod() + self.middleMethod()
    ";
    type_checker_test(code);
}

// ===== Mixed Access Patterns =====

#[test]
fn test_child_overrides_parent_method() {
    let code = "
class Animal
    fn speak() String
        \"sound\"

class Dog extends Animal
    fn speak() String
        \"bark\"
    fn test() String
        self.speak()
    ";
    type_checker_test(code);
}

#[test]
fn test_access_own_and_parent_fields() {
    let code = "
class Animal
    protected var name String

class Dog extends Animal
    var breed String
    fn describe() String
        self.name
    fn getBreed() String
        self.breed
    ";
    type_checker_test(code);
}

// ===== External Access (not via self) =====

#[test]
fn test_external_access_inherited_public_field() {
    let code = "
class Animal
    public var name String

class Dog extends Animal
    var breed String

class Vet
    fn checkName(d Dog) String
        d.name
    ";
    type_checker_test(code);
}

#[test]
fn test_external_access_inherited_protected_field_error() {
    let code = "
class Animal
    protected var age int

class Dog extends Animal
    var breed String

class Vet
    fn checkAge(d Dog) int
        d.age
    ";
    type_checker_error_test(code, "Protected and cannot be accessed");
}
