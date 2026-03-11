// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_struct_with_nested_collections() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

struct Complex
    id int
    names [String]

fn main()
    let names = List(["Alice", "Bob"])
    let c = Complex(id: 1, names: names)
    println(c.names[0])
    println(c.names[1])
    c.names.push("Charlie")
    println(f"{c.names.length()}")
    "#,
        "Alice\nBob\n3",
    );
}

#[test]
fn test_struct_equality_comparisons() {
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn main()
    let p1 = Point(x: 1, y: 2)
    let p2 = Point(x: 1, y: 2)
    println(f"{p1 == p2}")
    "#,
        "true",
    );
}
