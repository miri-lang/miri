// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_runs, assert_runs_with_output};

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
fn test_struct_field_access() {
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn main()
    let p = Point(x: 10, y: 20)
    print(f"{p.x + p.y}")
    "#,
        "30",
    );
}

#[test]
fn test_struct_field_mutation() {
    assert_runs_with_output(
        r#"
use system.io

struct Counter
    value int

fn main()
    var c = Counter(value: 0)
    c.value = 42
    print(f"{c.value}")
    "#,
        "42",
    );
}

#[test]
fn test_struct_multiple_fields() {
    assert_runs(
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
