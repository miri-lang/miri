// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A receiver typed as a concrete class runs the body the instance's own class
//! gives the method: a subclass override is reached through the vtable, and an
//! instance of the base itself still runs the base's body.

use super::utils::*;

#[test]
fn test_base_typed_variable_runs_the_subclass_override() {
    assert_runs_with_output(
        r#"
use system.io

class B
    fn m() int
        return 1

class C extends B
    fn m() int
        return 2

fn main()
    let b B = C()
    println(f"{b.m()}")
"#,
        "2",
    );
}

#[test]
fn test_abstract_receiver_over_a_concrete_base_runs_the_override() {
    assert_runs_with_output(
        r#"
use system.io

class B
    fn m() int
        return 1

abstract class A extends B
    abstract fn n() int

class C extends A
    fn m() int
        return 2
    fn n() int
        return 3

fn main()
    let a A = C()
    println(f"{a.m()} {a.n()}")
"#,
        "2 3",
    );
}

#[test]
fn test_base_typed_variable_holding_a_base_instance_runs_the_base_body() {
    assert_runs_with_output(
        r#"
use system.io

class B
    fn m() int
        return 1

class C extends B
    fn m() int
        return 2

fn main()
    let b B = B()
    let c B = C()
    println(f"{b.m()} {c.m()}")
"#,
        "1 2",
    );
}

#[test]
fn test_grandchild_without_override_runs_the_nearest_override() {
    assert_runs_with_output(
        r#"
use system.io

class B
    fn m() int
        return 1

class C extends B
    fn m() int
        return 2

class D extends C
    fn other() int
        return 9

fn main()
    let b B = D()
    let c C = D()
    println(f"{b.m()} {c.m()}")
"#,
        "2 2",
    );
}

#[test]
fn test_base_method_calling_self_reaches_the_subclass_override() {
    assert_runs_with_output(
        r#"
use system.io

class Shape
    fn name(self) String
        return "shape"
    fn describe(self) String
        return f"a {self.name()}"

class Circle extends Shape
    fn name(self) String
        return "circle"

fn main()
    println(Circle().describe())
    println(Shape().describe())
"#,
        "a circle\na shape",
    );
}

#[test]
fn test_base_typed_parameter_and_list_elements_dispatch_per_instance() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Animal
    fn sound(self) String
        return "..."

class Dog extends Animal
    fn sound(self) String
        return "woof"

class Cat extends Animal
    fn sound(self) String
        return "meow"

fn speak(a Animal) String
    return a.sound()

fn main()
    let animals = List<Animal>()
    animals.push(Dog())
    animals.push(Animal())
    animals.push(Cat())
    for a in animals
        println(speak(a))
"#,
        "woof\n...\nmeow",
    );
}

#[test]
fn test_override_holding_managed_fields_is_released_through_the_base() {
    assert_runs_with_output(
        r#"
use system.io

class B
    var label String
    fn init(label String)
        self.label = label
    fn show(self) String
        return self.label

class C extends B
    var extra String
    fn init(label String, extra String)
        super.init(label)
        self.extra = extra
    fn show(self) String
        return f"{self.label}+{self.extra}"

fn main()
    var b B = C("x", "y")
    println(b.show())
    b = B("z")
    println(b.show())
"#,
        "x+y\nz",
    );
}

#[test]
fn test_generic_base_typed_variable_runs_the_subclass_override() {
    assert_runs_with_output(
        r#"
use system.io

class Holder<T>
    var item T
    fn init(item T)
        self.item = item
    fn describe(self) String
        return "holder"

class IntHolder extends Holder<int>
    fn init(item int)
        super.init(item)
    fn describe(self) String
        return f"int {self.item}"

fn main()
    let h Holder<int> = IntHolder(7)
    println(h.describe())
"#,
        "int 7",
    );
}

#[test]
fn test_base_typed_variable_rejects_a_method_only_the_subclass_declares() {
    assert_compiler_error(
        r#"
class B
    fn m() int
        return 1

class C extends B
    fn only_c() int
        return 2

fn main()
    let b B = C()
    b.only_c()
"#,
        "only_c",
    );
}
