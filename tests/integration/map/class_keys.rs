// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Keys of a class that defines `equals` are matched through that method.
//!
//! A map decides whether two keys are the same key the way a set decides
//! whether two elements are the same element, so a class key answers the
//! question `==` answers for it rather than comparing addresses.

use super::utils::{assert_runs, assert_runs_with_output};

#[test]
fn constructed_map_matches_class_keys_through_their_equals() {
    assert_runs_with_output(
        r#"
use system.collections.map

class Point
    x int
    y int

    fn init(x int, y int)
        self.x = x
        self.y = y

    public fn equals(other Point) bool
        return self.x == other.x and self.y == other.y

fn main()
    var m = Map<Point, int>()
    m.set(Point(1, 2), 10)
    m.set(Point(1, 2), 20)
    m.set(Point(2, 1), 30)
    println(f"{m.length()},{m.contains_key(Point(2, 1))}")
    match m.get(Point(1, 2))
        Some(v): println(f"{v}")
        None: println("MISS")
"#,
        "2,true\n20",
    );
}

#[test]
fn map_literal_matches_class_keys_through_their_equals() {
    assert_runs_with_output(
        r#"
class Tag
    name String

    fn init(name String)
        self.name = name

    public fn equals(other Tag) bool
        return self.name == other.name

fn main()
    let m = {Tag("A".to_lower()): 1, Tag("B".to_lower()): 2}
    match m.get(Tag("B".to_lower()))
        Some(v): println(f"B={v}")
        None: println("MISS")
"#,
        "B=2",
    );
}

#[test]
fn map_with_class_keys_releases_replaced_and_removed_keys() {
    assert_runs(
        r#"
use system.collections.map

class Point
    x int

    fn init(x int)
        self.x = x

    public fn equals(other Point) bool
        return self.x == other.x

fn main()
    var m = Map<Point, int>()
    for i in 0..40
        m.set(Point(i % 4), i)
    m.remove(Point(1))
    m.clear()
    m.set(Point(7), 7)
"#,
    );
}
