// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::type_checker::utils::{type_checker_error_test, type_checker_test};

// ===== Valid Constructor Cases =====

#[test]
fn test_constructor_calls_super_init() {
    // Child class correctly calls super.init()
    let code = "
class Animal
    var name String
    public fn init(n String)
        self.name = n

class Dog extends Animal
    var breed String
    public fn init(n String, b String)
        super.init(n)
        self.breed = b
    ";
    type_checker_test(code);
}

#[test]
fn test_constructor_no_parent_init_needed() {
    // Parent has no init, child doesn't need to call super.init
    let code = "
class Animal
    var species String

class Dog extends Animal
    var breed String
    fn init(b String)
        self.breed = b
    ";
    type_checker_test(code);
}

#[test]
fn test_no_constructor_no_parent() {
    // Class without inheritance doesn't need super.init
    let code = "
class Animal
    var name String
    fn init(n String)
        self.name = n
    ";
    type_checker_test(code);
}

// ===== Invalid Constructor Cases =====

#[test]
fn test_constructor_missing_super_init_error() {
    // Child class must call super.init() when parent has init
    let code = "
class Animal
    var name String
    public fn init(n String)
        self.name = n

class Dog extends Animal
    var breed String
    fn init(b String)
        self.breed = b
    ";
    type_checker_error_test(code, "must call super.init");
}

#[test]
fn test_constructor_multi_level_missing_super_init_error() {
    // Even at multiple inheritance levels, super.init is required
    let code = "
class Animal
    var name String
    public fn init(n String)
        self.name = n

class Mammal extends Animal
    public fn init(n String)
        super.init(n)

class Dog extends Mammal
    var breed String
    fn init(b String)
        self.breed = b
    ";
    type_checker_error_test(code, "must call super.init");
}
