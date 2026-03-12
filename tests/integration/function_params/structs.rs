// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_struct_param_field_access() {
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn get_x(p Point) int
    p.x

fn main()
    let p = Point(x: 10, y: 20)
    let r = get_x(p)
    println(f"{r}")
"#,
        "10",
    );
}

#[test]
fn test_struct_param_sum_fields() {
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn manhattan(p Point) int
    p.x + p.y

fn main()
    let p = Point(x: 3, y: 4)
    let r = manhattan(p)
    println(f"{r}")
"#,
        "7",
    );
}

#[test]
fn test_struct_and_int_params() {
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn scale_x(p Point, factor int) int
    p.x * factor

fn main()
    let p = Point(x: 5, y: 0)
    let r = scale_x(p, 8)
    println(f"{r}")
"#,
        "40",
    );
}

#[test]
fn test_struct_return() {
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn make_point(x int, y int) Point
    Point(x: x, y: y)

fn main()
    let p = make_point(3, 7)
    println(f"{p.x}")
    println(f"{p.y}")
"#,
        "3\n7", // Original was "3" and "7"
    );
}
