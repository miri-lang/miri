// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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

#[test]
fn test_auto_copy_negative_managed_struct_aliasing() {
    // A struct with a managed field is NOT auto-copy (it's managed/RC'd).
    // Therefore, assignment creates an alias, and mutating one mutates the other.
    assert_runs_with_output(
        r#"
use system.io

struct ManagedRecord
    label String
    count int

fn main()
    let a = ManagedRecord(label: "shared", count: 10)
    var b = a // RC increment, NOT bitwise copy!

    b.count = 42

    // Since a and b point to the same managed object, a.count should be 42.
    println(f"{a.count}")
    "#,
        "42",
    );
}
