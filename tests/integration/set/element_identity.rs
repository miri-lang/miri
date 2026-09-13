// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which elements a set treats as the same element.
//!
//! A string element is matched by its content and an element of a class that
//! defines `equals` is matched through that method — the same answer `==`
//! gives. Every element compared here is built at run time: two equal string
//! literals share one pooled allocation, so a literal would match by address
//! and hide a set that never looks past the pointer.

use super::utils::{assert_runs, assert_runs_with_output};

#[test]
fn set_finds_a_string_built_at_run_time_by_its_content() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<String>()
    s.add("PEAR".to_lower())
    let pear = s.contains("PEAR".to_lower())
    let fig = s.contains("FIG".to_lower())
    println(f"{pear}")
    println(f"{fig}")
"#,
        "true\nfalse",
    );
}

#[test]
fn set_keeps_one_element_for_equal_string_content_added_twice() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<String>()
    let first = s.add("PEAR".to_lower())
    let second = s.add("PEAR".to_lower())
    println(f"{first},{second},{s.length()}")
"#,
        "true,false,1",
    );
}

#[test]
fn set_removes_a_string_named_by_equal_content() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<String>()
    s.add("PEAR".to_lower())
    let removed = s.remove("PEAR".to_lower())
    let still_held = s.contains("PEAR".to_lower())
    println(f"{removed},{s.length()},{still_held}")
"#,
        "true,0,false",
    );
}

#[test]
fn in_operator_finds_a_string_built_at_run_time_by_its_content() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<String>()
    s.add("PEAR".to_lower())
    let probe = "PEAR".to_lower()
    if probe in s
        println("in=yes")
    else
        println("in=no")
"#,
        "in=yes",
    );
}

#[test]
fn set_literal_of_run_time_strings_matches_by_content() {
    assert_runs_with_output(
        r#"
fn main()
    let a = "PEAR".to_lower()
    let b = "PEAR".to_lower()
    let s = {a, b}
    let held = s.contains("PEAR".to_lower())
    println(f"{s.length()},{held}")
"#,
        "1,true",
    );
}

#[test]
fn set_matches_a_pooled_literal_against_equal_run_time_content() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<String>()
    s.add("pear")
    let held = s.contains("PEAR".to_lower())
    let added = s.add("PEAR".to_lower())
    println(f"{held},{added},{s.length()}")
"#,
        "true,false,1",
    );
}

#[test]
fn set_of_strings_keeps_content_matching_across_growth() {
    // Enough distinct elements to rehash the table several times; each one is
    // then added again under a fresh allocation and must be recognised.
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<String>()
    for i in 0..200
        s.add(f"word{i}")
    for i in 0..200
        s.add(f"word{i}")
    var found = 0
    for i in 0..200
        if s.contains(f"word{i}")
            found = found + 1
    println(f"{s.length()},{found}")
"#,
        "200,200",
    );
}

#[test]
fn set_copy_matches_strings_by_content_after_it_separates() {
    // Adding to a shared set clones it first; the clone has to keep comparing
    // by content, or the duplicate lands as a second element.
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<String>()
    s.add("PEAR".to_lower())
    var t = s
    t.add("PEAR".to_lower())
    t.add("FIG".to_lower())
    let held = t.contains("PEAR".to_lower())
    println(f"{s.length()},{t.length()},{held}")
"#,
        "1,2,true",
    );
}

#[test]
fn set_of_strings_passed_to_a_function_matches_by_content() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn seen(s Set<String>, word String) bool
    return s.contains(word.to_lower())

fn main()
    var s = Set<String>()
    s.add("PEAR".to_lower())
    let pear = seen(s, "PEAR")
    let fig = seen(s, "FIG")
    println(f"{pear},{fig}")
"#,
        "true,false",
    );
}

#[test]
fn set_of_a_class_matches_elements_through_its_equals() {
    assert_runs_with_output(
        r#"
use system.collections.set

class Point
    x int
    y int

    fn init(x int, y int)
        self.x = x
        self.y = y

    public fn equals(other Point) bool
        return self.x == other.x and self.y == other.y

fn main()
    var s = Set<Point>()
    let first = s.add(Point(1, 2))
    let second = s.add(Point(1, 2))
    s.add(Point(3, 4))
    println(f"{first},{second},{s.length()}")
    println(f"{s.contains(Point(1, 2))},{s.contains(Point(2, 1))}")
    println(f"{s.remove(Point(3, 4))},{s.length()}")
"#,
        "true,false,2\ntrue,false\ntrue,1",
    );
}

#[test]
fn set_literal_of_a_class_matches_elements_through_its_equals() {
    assert_runs_with_output(
        r#"
class Tag
    name String

    fn init(name String)
        self.name = name

    public fn equals(other Tag) bool
        return self.name == other.name

fn main()
    let s = {Tag("A".to_lower()), Tag("A".to_lower()), Tag("B".to_lower())}
    let held = s.contains(Tag("A".to_lower()))
    println(f"{s.length()},{held}")
"#,
        "2,true",
    );
}

#[test]
fn set_of_a_generic_class_matches_through_the_equals_of_its_instantiation() {
    // The element's `equals` compares its `String` payload, so it has to be
    // the body compiled for `String` — the shared one would compare addresses.
    assert_runs_with_output(
        r#"
use system.collections.set

class Tagged<T>
    value T

    fn init(value T)
        self.value = value

    public fn equals(other Self) bool
        return self.value == other.value

fn main()
    let first = Tagged<String>("PEAR".to_lower())
    let second = Tagged<String>("PEAR".to_lower())
    let probe = Tagged<String>("PEAR".to_lower())
    var s = Set<Tagged<String>>()
    s.add(first)
    s.add(second)
    let held = s.contains(probe)
    println(f"{s.length()},{held}")
"#,
        "1,true",
    );
}

#[test]
fn set_of_a_class_without_equals_keeps_distinct_instances_apart() {
    // With no `equals`, two instances are the same element only when they are
    // the same instance — which is also what `==` answers for them.
    assert_runs_with_output(
        r#"
use system.collections.set

class Cell
    v int

    fn init(v int)
        self.v = v

fn main()
    var s = Set<Cell>()
    let c = Cell(1)
    s.add(c)
    s.add(Cell(1))
    println(f"{s.length()},{s.contains(c)}")
"#,
        "2,true",
    );
}

#[test]
fn set_of_a_class_with_equals_releases_what_it_holds() {
    // The leak check runs with every test; a rejected duplicate and a removed
    // element must both be released exactly once.
    assert_runs(
        r#"
use system.collections.set

class Point
    x int

    fn init(x int)
        self.x = x

    public fn equals(other Point) bool
        return self.x == other.x

fn main()
    var s = Set<Point>()
    for i in 0..50
        s.add(Point(i % 5))
    s.remove(Point(2))
    s.clear()
    s.add(Point(9))
"#,
    );
}
