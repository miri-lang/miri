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

#[test]
fn test_struct_returned_from_function() {
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn make_point(x int, y int) Point
    Point(x: x, y: y)

fn main()
    let p = make_point(x: 5, y: 10)
    println(f"{p.x}")
    println(f"{p.y}")
    "#,
        "5\n10",
    );
}

#[test]
fn test_struct_mixed_types_field_access() {
    assert_runs_with_output(
        r#"
use system.io

struct Record
    name string
    count int
    active bool

fn main()
    let r = Record(name: "hello", count: 42, active: true)
    println(r.name)
    println(f"{r.count}")
    println(f"{r.active}")
    "#,
        "hello\n42\ntrue",
    );
}

#[test]
fn test_struct_passed_and_returned() {
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn offset_point(p Point, dx int, dy int) Point
    Point(x: p.x + dx, y: p.y + dy)

fn main()
    let p = Point(x: 1, y: 2)
    let q = offset_point(p: p, dx: 10, dy: 20)
    println(f"{q.x}")
    println(f"{q.y}")
    "#,
        "11\n22",
    );
}

#[test]
fn test_struct_nested_function_calls() {
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn make_point(x int, y int) Point
    Point(x: x, y: y)

fn add_points(a Point, b Point) Point
    make_point(x: a.x + b.x, y: a.y + b.y)

fn main()
    let result = add_points(a: make_point(x: 1, y: 2), b: make_point(x: 3, y: 4))
    println(f"{result.x}")
    println(f"{result.y}")
    "#,
        "4\n6",
    );
}
