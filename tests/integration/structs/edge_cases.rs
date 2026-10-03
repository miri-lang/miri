// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_struct_with_nested_collections() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Complex
    id int
    names [String]

fn main()
    let names = List(["Alice", "Bob"])
    let c = Complex(id: 1, names: names)
    println(c.names[0])
    println(c.names[1])
    c.names.push("Charlie")
    println(f"{c.names.length()}")
    "#,
        "Alice\nBob\n3",
    );
}

#[test]
fn test_struct_equality_comparisons() {
    assert_runs_with_output(
        r#"

struct Point
    x int
    y int

fn main()
    let p1 = Point(x: 1, y: 2)
    let p2 = Point(x: 1, y: 2)
    println(f"{p1 == p2}")
    "#,
        "true",
    );
}

#[test]
fn test_struct_non_drop_method_is_rejected() {
    // Structs hold data only: a method is a clear compile error naming the
    // class it should be, not an internal compiler error at code generation.
    assert_compiler_error(
        r#"
struct P
    v int
    fn get() int
        return self.v

fn main()
    let p = P(v: 42)
"#,
        "Struct 'P' cannot define method 'get': a struct holds data only",
    );
}

#[test]
fn test_struct_drop_method_is_rejected_with_the_class_it_should_be() {
    // A type that runs code when it is released is a resource, and a resource
    // is a class.
    assert_compiler_error(
        r#"
struct Res
    id int
    fn drop(self)
        println("dropped")

fn main()
    let r = Res(id: 1)
    println(f"id={r.id}")
"#,
        "make 'Res' a class to give it a drop hook",
    );
}
