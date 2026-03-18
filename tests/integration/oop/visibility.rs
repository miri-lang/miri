// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ===== Public access =====

#[test]
fn test_public_field_readable_externally() {
    assert_runs_with_output(
        r#"
use system.io

class Animal
    public var name String
    fn init(n String)
        self.name = n

fn main()
    let a = Animal(n: "Cat")
    println(a.name)
    "#,
        "Cat",
    );
}

#[test]
fn test_public_method_callable_externally() {
    assert_runs_with_output(
        r#"
use system.io

class Animal
    public fn sound() String
        "roar"

fn main()
    let a = Animal()
    println(a.sound())
    "#,
        "roar",
    );
}

// ===== Private access — happy path (within declaring class) =====

#[test]
fn test_private_field_accessible_within_same_class() {
    assert_runs_with_output(
        r#"
use system.io

class BankAccount
    private var balance int
    fn init(b int)
        self.balance = b
    fn getBalance() int
        self.balance

fn main()
    let acc = BankAccount(b: 100)
    println(f"{acc.getBalance()}")
    "#,
        "100",
    );
}

#[test]
fn test_private_method_callable_within_same_class() {
    assert_runs_with_output(
        r#"
use system.io

class Calculator
    private fn double(x int) int
        x * 2
    fn compute(x int) int
        self.double(x)

fn main()
    let c = Calculator()
    println(f"{c.compute(5)}")
    "#,
        "10",
    );
}

// ===== Protected access — happy path (subclass via self) =====

#[test]
fn test_protected_field_accessible_in_subclass() {
    assert_runs_with_output(
        r#"
use system.io

class Animal
    protected var age int
    fn init(a int)
        self.age = a

class Dog extends Animal
    fn yearsOld() int
        self.age

fn main()
    let d = Dog(a: 3)
    println(f"{d.yearsOld()}")
    "#,
        "3",
    );
}

#[test]
fn test_protected_method_callable_in_subclass() {
    assert_runs_with_output(
        r#"
use system.io

class Animal
    protected fn describe() String
        "animal"

class Dog extends Animal
    fn info() String
        self.describe()

fn main()
    let d = Dog()
    println(d.info())
    "#,
        "animal",
    );
}

#[test]
fn test_protected_field_accessible_in_deep_subclass() {
    assert_runs_with_output(
        r#"
use system.io

class Animal
    protected var name String
    fn init(n String)
        self.name = n

class Mammal extends Animal

class Dog extends Mammal
    fn getName() String
        self.name

fn main()
    let d = Dog(n: "Rex")
    println(d.getName())
    "#,
        "Rex",
    );
}

// ===== Error cases =====

#[test]
fn test_private_field_inaccessible_from_subclass() {
    assert_compiler_error(
        r#"
class Animal
    private var secret int

class Dog extends Animal
    fn getSecret() int
        self.secret
    "#,
        "Private and cannot be accessed",
    );
}

#[test]
fn test_private_field_inaccessible_from_external_class() {
    assert_compiler_error(
        r#"
class Person
    private var age int

class Snooper
    fn spy(p Person) int
        p.age
    "#,
        "Private and cannot be accessed",
    );
}

#[test]
fn test_private_method_inaccessible_from_external_class() {
    assert_compiler_error(
        r#"
class Vault
    private fn unlock() int
        42

class Thief
    fn tryUnlock(v Vault) int
        v.unlock()
    "#,
        "Private and cannot be accessed",
    );
}

#[test]
fn test_protected_field_inaccessible_from_external_class() {
    assert_compiler_error(
        r#"
class Person
    protected var ssn String

class Hacker
    fn steal(p Person) String
        p.ssn
    "#,
        "Protected and cannot be accessed",
    );
}

#[test]
fn test_protected_method_inaccessible_from_external_class() {
    assert_compiler_error(
        r#"
class Animal
    protected fn internalSound() String
        "growl"

class Stranger
    fn call(a Animal) String
        a.internalSound()
    "#,
        "Protected and cannot be accessed",
    );
}

// ── Constructor visibility ─────────────────────────────────────────────────────

#[test]
fn test_private_init_still_constructible_within_class() {
    // A private `init` is callable through the constructor syntax from outside
    // because the constructor syntax is the canonical way to build objects.
    // Document the actual behavior: either it works or gives a clear error.
    assert_runs_with_output(
        r#"
use system.io

class Token
    private var value int
    private fn init(v int)
        self.value = v
    fn getValue() int
        self.value

fn main()
    let t = Token(v: 7)
    println(f"{t.getValue()}")
    "#,
        "7",
    );
}

#[test]
fn test_protected_field_not_accessible_via_sibling_class() {
    // Two classes share the same parent but are not in a subtype relationship.
    // Cat is not a subtype of Dog, so Cat must NOT read Dog's protected field
    // via an instance parameter `d Dog`.
    assert_compiler_error(
        r#"
class Animal
    protected var dna String

class Dog extends Animal

class Cat extends Animal
    fn sniff(d Dog) String
        d.dna
    "#,
        "Protected and cannot be accessed",
    );
}

#[test]
fn test_private_field_inaccessible_from_subclass_method() {
    // A subclass method must NOT read a private field of its parent via `self`.
    assert_compiler_error(
        r#"
class Vehicle
    private var vin String

class Car extends Vehicle
    fn getVin() String
        self.vin
    "#,
        "Private and cannot be accessed",
    );
}

#[test]
fn test_visibility_defaults_to_public() {
    // A field declared without an explicit modifier is accessible externally
    // (default visibility is public in Miri).
    assert_runs_with_output(
        r#"
use system.io

class Box
    var side int

fn main()
    let b = Box(side: 5)
    println(f"{b.side}")
    "#,
        "5",
    );
}
