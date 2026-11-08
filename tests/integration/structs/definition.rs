// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_struct_definition() {
    assert_runs(
        r#"
struct Point
    x int
    y int

fn main()
    let p = Point(x: 10, y: 20)
    "#,
    );
}

#[test]
fn test_struct_multiple_fields() {
    assert_runs(
        r#"
struct Person
    name String
    age int
    active bool

fn main()
    let p = Person(name: "Alice", age: 30, active: true)
    "#,
    );
}

#[test]
fn test_empty_struct() {
    assert_compiler_error(
        r#"
struct Empty

fn main()
    let e = Empty()
    "#,
        "Missing Struct Members",
    );
}

#[test]
fn test_infinite_recursive_struct() {
    assert_compiler_error(
        r#"
struct Node
    value int
    next Node

fn main()
    let x = 1
    "#,
        "Infinite recursive type",
    );
}

/// The refusal names a generic trait as it is written, arguments included.
#[test]
fn test_struct_naming_a_generic_trait_is_refused_with_the_trait_as_written() {
    assert_compiler_error(
        r#"
trait Container<T>
    fn get() T

struct S implements Container<int>
    p int

fn main()
    let s = S(p: 1)
"#,
        "make 'S' a class to implement 'Container<int>'",
    );
}
