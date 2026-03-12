// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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
fn test_struct_lvalue_mutation_assignment() {
    assert_compiler_error(
        r#"
struct Point
    x int
    y int

fn make_point() Point
    Point(x: 0, y: 0)

fn main()
    make_point().x = 10
    "#,
        "Cannot assign to field of immutable variable",
    );
}

#[test]
fn test_struct_instantiation_field_errors() {
    assert_compiler_error(
        r#"
struct Point
    x int
    y int

fn main()
    let p = Point(x: 10)
    "#,
        "Missing argument for field 'y'",
    );

    assert_compiler_error(
        r#"
struct Point
    x int
    y int

fn main()
    let p = Point(x: 10, y: 20, z: 30)
    "#,
        "Unknown field 'z'",
    );
}
