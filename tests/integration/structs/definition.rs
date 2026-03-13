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
