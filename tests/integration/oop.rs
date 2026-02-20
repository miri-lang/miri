// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_runs, assert_runs_with_output};

#[test]
fn test_class_definition() {
    assert_runs(
        r#"
class Counter
    var count int = 0

fn main()
    let c = Counter()
    "#,
    );
}

#[test]
fn test_class_instantiation() {
    assert_runs(
        r#"
class Point
    var x int = 0
    var y int = 0

fn main()
    let p = Point()
    "#,
    );
}

#[test]
fn test_class_with_constructor_args() {
    assert_runs(
        r#"
class Point
    var x int
    var y int

fn main()
    let p = Point(x: 10, y: 20)
    "#,
    );
}

#[test]
#[ignore = "Type checker: class fields are private by default"]
fn test_class_field_access() {
    assert_runs_with_output(
        r#"
use system.io

class Point
    var x int = 10
    var y int = 20

fn main()
    let p = Point()
    println(p.x + p.y)
    "#,
        "30",
    );
}
