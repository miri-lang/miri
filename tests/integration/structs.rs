// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{interpreter_assert_returns, interpreter_assert_runs};

#[test]
fn test_struct_definition() {
    interpreter_assert_runs(
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
fn test_struct_field_access() {
    interpreter_assert_returns(
        r#"
struct Point
    x int
    y int

fn main() int
    let p = Point(x: 10, y: 20)
    p.x + p.y
    "#,
        30,
    );
}

#[test]
fn test_struct_field_mutation() {
    interpreter_assert_returns(
        r#"
struct Counter
    value int

fn main() int
    var c = Counter(value: 0)
    c.value = 42
    c.value
    "#,
        42,
    );
}

#[test]
fn test_struct_multiple_fields() {
    interpreter_assert_runs(
        r#"
struct Person
    name string
    age int
    active bool

fn main()
    let p = Person(name: "Alice", age: 30, active: true)
    "#,
    );
}
