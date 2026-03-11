// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_array_for_loop() {
    assert_runs_with_output(
        r#"
use system.io

for x in [1, 2, 3]
    println(f"{x}")
    "#,
        "1\n2\n3\n",
    );
}

#[test]
fn test_array_for_loop_strings() {
    assert_runs_with_output(
        r#"
use system.io

for s in ["a", "b"]
    println(s)
    "#,
        "a\nb\n",
    );
}

#[test]
fn test_array_returned_from_function() {
    assert_runs_with_output(
        r#"
use system.io

fn make_array() [int; 3]
    [10, 20, 30]

fn main()
    let a = make_array()
    println(f"{a[0]} {a[1]} {a[2]}")
"#,
        "10 20 30",
    );
}

#[test]
fn test_array_in_struct_field() {
    assert_runs_with_output(
        r#"
use system.io

struct Data
    values [int; 3]

fn main()
    let d = Data(values: [10, 20, 30])
    println(f"{d.values[0]} {d.values[1]} {d.values[2]}")
"#,
        "10 20 30",
    );
}

#[test]
fn test_array_of_structs_elem_size() {
    // Array<Point> where Point is a struct — elements are pointer-sized
    // because structs are heap-allocated.
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn main()
    let p1 = Point(x: 1, y: 2)
    let p2 = Point(x: 3, y: 4)
    let arr = [p1, p2]
    let first = arr[0]
    println(f"{first.x}")
    println(f"{first.y}")
    let second = arr[1]
    println(f"{second.x}")
    "#,
        "1\n2\n3",
    );
}

#[test]
fn test_nested_arrays() {
    // Nested arrays: inner arrays are pointer-sized elements.
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let a = [10, 20, 30]
    let b = [40, 50, 60]
    let nested = [a, b]
    let inner = nested[1]
    println(f"{inner[2]}")
    "#,
        "60",
    );
}
