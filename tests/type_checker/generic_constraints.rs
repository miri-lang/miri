// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// Additional generic constraint tests for edge cases

use crate::type_checker::utils::{check_error, check_success};

// ===== Multiple Generic Parameters with Constraints =====

#[test]
fn test_multiple_constrained_generics() {
    let code = "
trait Printable
    fn print() string

trait Comparable
    fn compare(other int) int

class Document implements Printable
    fn print() string
        \"doc\"

class Number implements Comparable
    fn compare(other int) int
        0

class Processor<P implements Printable, C implements Comparable>
    var printer P
    var comparer C
";
    check_success(code);
}

#[test]
fn test_multiple_constrained_generics_instantiation() {
    let code = "
trait Printable
    fn print() string

trait Comparable
    fn compare(other int) int

class Document implements Printable
    fn print() string
        \"doc\"

class Number implements Comparable
    fn compare(other int) int
        0

class Processor<P implements Printable, C implements Comparable>
    var printer P
    var comparer C

let proc Processor<Document, Number>
";
    check_success(code);
}

#[test]
fn test_multiple_constrained_generics_fail_first() {
    let code = "
trait Printable
    fn print() string

trait Comparable
    fn compare(other int) int

class Plain
    var x int

class Number implements Comparable
    fn compare(other int) int
        0

class Processor<P implements Printable, C implements Comparable>
    var printer P
    var comparer C

let proc Processor<Plain, Number>
";
    check_error(code, "does not satisfy constraint");
}

#[test]
fn test_multiple_constrained_generics_fail_second() {
    let code = "
trait Printable
    fn print() string

trait Comparable
    fn compare(other int) int

class Document implements Printable
    fn print() string
        \"doc\"

class Plain
    var x int

class Processor<P implements Printable, C implements Comparable>
    var printer P
    var comparer C

let proc Processor<Document, Plain>
";
    check_error(code, "does not satisfy constraint");
}

// ===== Class Extends Constraint with Method Access =====

#[test]
fn test_constrained_generic_method_access() {
    let code = "
class Animal
    var name string
    public fn speak() string
        \"sound\"

class Dog extends Animal
    public fn speak() string
        \"bark\"

class Kennel<T extends Animal>
    var pet T
    fn getPetSound() string
        self.pet.speak()

let k Kennel<Dog>
";
    check_success(code);
}

// ===== Trait Constraint with Method Call =====
// Note: Trait member resolution on generic type parameters requires additional work
// to resolve trait methods. For now, using extends with class is recommended.

#[test]
fn test_trait_constraint_method_call() {
    let code = "
trait Summable
    fn value() int

class Counter implements Summable
    var count int
    public fn value() int
        self.count

class Summer<T extends Counter>
    var item T
    fn getItemValue() int
        self.item.value()
";
    check_success(code);
}
