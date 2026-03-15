// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_init_body_executes() {
    assert_runs_with_output(
        r#"
use system.io

class Greeter
    var name String
    fn init(n String)
        self.name = n
        println("constructed")

fn main()
    let g = Greeter(n: "world")
    println(g.name)
    "#,
        "constructed\nworld",
    );
}

#[test]
fn test_init_with_self_field_assignment() {
    assert_runs_with_output(
        r#"
use system.io

class Dog
    var name String
    var age int
    fn init(n String, a int)
        self.name = n
        self.age = a

fn main()
    let d = Dog(n: "Rex", a: 5)
    println(f"{d.name} is {d.age} years old")
    "#,
        "Rex is 5 years old",
    );
}

#[test]
fn test_init_with_logic() {
    assert_runs_with_output(
        r#"
use system.io

class Counter
    var count int = 0
    fn init(start int)
        self.count = start * 2

fn main()
    let c = Counter(start: 3)
    println(f"{c.count}")
    "#,
        "6",
    );
}

#[test]
fn test_class_without_init_still_works() {
    assert_runs_with_output(
        r#"
use system.io

class Point
    var x int
    var y int

fn main()
    let p = Point(x: 10, y: 20)
    println(f"{p.x + p.y}")
    "#,
        "30",
    );
}

// NOTE: super.init() chaining requires inherited field layout support
// in codegen, which is not yet implemented. The type checker validates
// super.init() calls, and the MIR lowering emits the correct call,
// but field offsets for inherited fields are not resolved correctly.
// Uncomment when inherited field layout is implemented.
// TODO: this also needs member visibility support.
//
// #[test]
// fn test_super_init_chaining() {
//     assert_runs_with_output(
//         r#"
// use system.io
//
// class Animal
//     var name String
//     public fn init(n String)
//         self.name = n
//         println("Animal init")
//
// class Dog extends Animal
//     var breed String
//     public fn init(n String, b String)
//         super.init(n)
//         self.breed = b
//         println("Dog init")
//
// fn main()
//     let d = Dog(n: "Rex", b: "Lab")
//     println(d.breed)
//     "#,
//         "Animal init\nDog init\nLab",
//     );
// }
