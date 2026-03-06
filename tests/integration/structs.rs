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
    name String
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
    name String
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

// ==================== Auto-Copy Detection Tests ====================

#[test]
fn test_auto_copy_struct_assignment() {
    // Point is auto-copy (all primitive fields, small size).
    // Assignment produces a bitwise copy — both variables are usable.
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x float
    y float

fn main()
    let p = Point(x: 1.0, y: 2.0)
    let q = p
    println(f"{p.x}")
    println(f"{q.x}")
    "#,
        "1.0\n1.0",
    );
}

#[test]
fn test_auto_copy_struct_pass_to_function() {
    // Auto-copy struct: passing to a function copies — original still usable.
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn distance_sq(p Point) int
    p.x * p.x + p.y * p.y

fn main()
    let p = Point(x: 3, y: 4)
    let d = distance_sq(p: p)
    println(f"{d}")
    println(f"{p.x}")
    "#,
        "25\n3",
    );
}

#[test]
fn test_auto_copy_nested_struct() {
    // A struct of auto-copy structs is itself auto-copy.
    assert_runs_with_output(
        r#"
use system.io

struct Vec2
    x int
    y int

struct Rect
    origin Vec2
    size Vec2

fn main()
    let r = Rect(origin: Vec2(x: 1, y: 2), size: Vec2(x: 10, y: 20))
    let s = r
    println(f"{r.origin.x}")
    println(f"{s.size.y}")
    "#,
        "1\n20",
    );
}

// ==================== Drop Specialization Tests ====================

#[test]
fn test_drop_struct_with_string_field() {
    // A struct containing a String field is NOT auto-copy. It is managed
    // (heap-allocated with RC). When dropped, the String field must be DecRef'd.
    assert_runs_with_output(
        r#"
use system.io

struct Named
    label String
    value int

fn main()
    let n = Named(label: "hello", value: 42)
    println(n.label)
    println(f"{n.value}")
    "#,
        "hello\n42",
    );
}

#[test]
fn test_drop_struct_with_managed_fields_in_scope() {
    // Struct with a String field is created inside a function, goes out of scope,
    // and the nested String must be released correctly (no leak, no crash).
    assert_runs_with_output(
        r#"
use system.io

struct Record
    name String
    count int

fn make_and_use() int
    let r = Record(name: "temp", count: 99)
    r.count

fn main()
    let c = make_and_use()
    println(f"{c}")
    "#,
        "99",
    );
}

#[test]
fn test_drop_struct_with_array_field() {
    // A struct containing an array field: both the struct and the array
    // must be freed correctly on drop.
    assert_runs_with_output(
        r#"
use system.io

struct Container
    data [int; 3]
    label String

fn main()
    let c = Container(data: [10, 20, 30], label: "test")
    println(c.label)
    "#,
        "test",
    );
}
