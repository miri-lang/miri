// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::type_checker::utils::{check_error, check_success};

// ===== Inherited Field Access =====

#[test]
fn test_access_parent_public_field_via_self() {
    let code = "
class Animal
    public var name string

class Dog extends Animal
    fn getName() string
        self.name
    ";
    check_success(code);
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
    check_success(code);
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
    check_error(code, "Private and cannot be accessed");
}

// ===== Inherited Method Access =====

#[test]
fn test_access_parent_public_method_via_self() {
    let code = "
class Animal
    public fn speak() string
        \"sound\"

class Dog extends Animal
    fn bark() string
        self.speak()
    ";
    check_success(code);
}

#[test]
fn test_access_parent_protected_method_via_self() {
    let code = "
class Animal
    protected fn getInfo() string
        \"info\"

class Dog extends Animal
    fn info() string
        self.getInfo()
    ";
    check_success(code);
}

#[test]
fn test_access_parent_private_method_via_self_error() {
    let code = "
class Animal
    private fn secretMethod() string
        \"secret\"

class Dog extends Animal
    fn tryAccess() string
        self.secretMethod()
    ";
    check_error(code, "Private and cannot be accessed");
}

// ===== Multi-level Inheritance =====

#[test]
fn test_multi_level_field_access() {
    let code = "
class Animal
    protected var name string

class Mammal extends Animal
    protected var legs int

class Dog extends Mammal
    fn describe() string
        self.name
    ";
    check_success(code);
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
    check_success(code);
}

// ===== Mixed Access Patterns =====

#[test]
fn test_child_overrides_parent_method() {
    let code = "
class Animal
    fn speak() string
        \"sound\"

class Dog extends Animal
    fn speak() string
        \"bark\"
    fn test() string
        self.speak()
    ";
    check_success(code);
}

#[test]
fn test_access_own_and_parent_fields() {
    let code = "
class Animal
    protected var name string

class Dog extends Animal
    var breed string
    fn describe() string
        self.name
    fn getBreed() string
        self.breed
    ";
    check_success(code);
}

// ===== External Access (not via self) =====

#[test]
fn test_external_access_inherited_public_field() {
    let code = "
class Animal
    public var name string

class Dog extends Animal
    var breed string

class Vet
    fn checkName(d Dog) string
        d.name
    ";
    check_success(code);
}

#[test]
fn test_external_access_inherited_protected_field_error() {
    let code = "
class Animal
    protected var age int

class Dog extends Animal
    var breed string

class Vet
    fn checkAge(d Dog) int
        d.age
    ";
    check_error(code, "Protected and cannot be accessed");
}
