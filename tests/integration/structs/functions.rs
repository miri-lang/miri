// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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
