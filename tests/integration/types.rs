// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::test_utils::{assert_invalid, assert_valid};

#[test]
fn test_struct_definition_and_usage() {
    assert_valid(
        r#"
struct Point
    x int
    y int

fn main()
    let p = Point(x: 1, y: 2)
    print(p.x)
    print(p.y)
"#,
    );
}

#[test]
fn test_enum_definition_and_usage() {
    assert_valid(
        r#"
enum Color
    Red
    Green
    Blue

fn main()
    let c = Color.Red
    match c
        Color.Red: print("Red")
        Color.Green: print("Green")
        Color.Blue: print("Blue")
"#,
    );
}

#[test]
fn test_generic_struct() {
    assert_valid(
        r#"
struct Box<T>
    value T

fn main()
    let b = Box(value: 10)
    print(b.value)
"#,
    );
}

#[test]
fn test_invalid_struct_field_access() {
    assert_invalid(
        r#"
struct Point
    x int
    y int

fn main()
    let p = Point(x: 1, y: 2)
    print(p.z)
"#,
        &["Type 'Point' has no field 'z'"],
    );
}

#[test]
fn test_invalid_struct_initialization() {
    assert_invalid(
        r#"
struct Point
    x int
    y int

fn main()
    let p = Point(x: 1)
"#,
        &["Missing argument for field 'y'"],
    );
}
