// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `hash()` agrees with `==`: a type whose equality is derived derives its
//! hash from the same parts, a type that writes its own `equals` writes a
//! `hash` beside it through `Hashable`, and one that does not is refused
//! wherever it would be hashed.

use super::utils::*;

const POINT_WITHOUT_HASH: &str = r#"
use system.io
use system.collections.set
use system.collections.map
use system.collections.list

class Point implements Equatable
    x int
    y int
    fn equals(other Point) bool
        return self.x == other.x and self.y == other.y
"#;

const POINT_WITH_HASH: &str = r#"
use system.io
use system.collections.set
use system.collections.map

class Point implements Equatable, Hashable
    x int
    y int
    label String
    fn equals(other Point) bool
        return self.x == other.x and self.y == other.y
    fn hash() int
        return hash_combine(self.x.hash(), self.y.hash())
"#;

const MISSING_HASH: &str = "'Point' defines its own 'equals' but no 'hash' consistent with it";

fn with(declarations: &str, main: &str) -> String {
    format!("{declarations}\n{main}")
}

#[test]
fn equal_values_of_derived_equality_hash_alike() {
    assert_heap_guard_output(
        r#"
use system.collections.list

struct P
    x int
    s String

enum E
    A(int)
    B(String)
    C

fn main()
    let negative_zero = -0.0
    let zero = 0.0
    var some int? = 3
    var other int? = 3
    println(f"{P(x: 1, s: 'a' + 'b').hash() == P(x: 1, s: 'ab').hash()}")
    println(f"{E.B('q' + 'r').hash() == E.B('qr').hash()} {E.A(1).hash() == E.C.hash()}")
    println(f"{negative_zero.hash() == zero.hash()} {some.hash() == other.hash()}")
    println(f"{List([1, 2]).hash() == List([1, 2]).hash()} {List([1, 2]).hash() == List([2, 1]).hash()}")
"#,
        "true\ntrue false\ntrue true\ntrue false",
    );
}

#[test]
fn a_generic_body_hashes_its_parameter_at_the_bound_type() {
    assert_runs_with_output(
        r#"
fn hashed<T>(value T) int
    return value.hash()

fn main()
    println(f"{hashed(3) == 3.hash()} {hashed('ab') == 'ab'.hash()}")
"#,
        "true true",
    );
}

#[test]
fn hash_combine_depends_on_the_order_of_its_parts() {
    assert_runs_with_output(
        r#"
fn main()
    println(f"{hash_combine(1, 2) == hash_combine(1, 2)} {hash_combine(1, 2) == hash_combine(2, 1)}")
"#,
        "true false",
    );
}

#[test]
fn a_set_places_a_hashable_class_by_its_hash_and_matches_it_by_equals() {
    assert_heap_guard_output(
        &with(
            POINT_WITH_HASH,
            r#"
fn main()
    let points = Set<Point>()
    points.add(Point(x: 1, y: 2, label: 'a'))
    points.add(Point(x: 1, y: 2, label: 'b'))
    points.add(Point(x: 2, y: 1, label: 'a'))
    println(f"{points.length()} {points.contains(Point(x: 2, y: 1, label: 'z'))}")
"#,
        ),
        "2 true",
    );
}

#[test]
fn a_map_keyed_by_a_hashable_class_finds_an_equal_key() {
    assert_heap_guard_output(
        &with(
            POINT_WITH_HASH,
            r#"
fn main()
    var names = Map<Point, String>()
    names[Point(x: 1, y: 2, label: 'a')] = 'first'
    names[Point(x: 1, y: 2, label: 'b')] = 'second'
    println(f"{names.length()} {names[Point(x: 1, y: 2, label: 'c')]}")
"#,
        ),
        "1 second",
    );
}

#[test]
fn a_set_of_many_hashable_elements_holds_each_once() {
    assert_heap_guard_output(
        &with(
            POINT_WITH_HASH,
            r#"
fn main()
    let points = Set<Point>()
    var i = 0
    while i < 3000
        points.add(Point(x: i % 1000, y: i % 1000, label: 'p'))
        i += 1
    println(f"{points.length()}")
"#,
        ),
        "1000",
    );
}

#[test]
fn a_set_of_lists_matches_and_places_them_by_their_elements() {
    assert_heap_guard_output(
        r#"
use system.collections.set
use system.collections.list

fn main()
    let lists = Set<List<String>>()
    lists.add(List(['a' + 'b', 'c']))
    lists.add(List(['ab', 'c']))
    lists.add(List(['c', 'ab']))
    println(f"{lists.length()}")
"#,
        "2",
    );
}

#[test]
fn a_class_with_equals_and_no_hash_is_refused_as_a_set_element() {
    assert_compiler_error(
        &with(
            POINT_WITHOUT_HASH,
            r#"
fn main()
    let points = Set<Point>()
    points.add(Point(x: 1, y: 2))
"#,
        ),
        MISSING_HASH,
    );
}

#[test]
fn a_class_with_equals_and_no_hash_is_refused_as_a_map_key() {
    assert_compiler_error(
        &with(
            POINT_WITHOUT_HASH,
            r#"
fn main()
    var names = Map<Point, String>()
    names[Point(x: 1, y: 2)] = 'a'
"#,
        ),
        MISSING_HASH,
    );
}

#[test]
fn a_class_with_equals_and_no_hash_is_refused_where_its_hash_is_called() {
    assert_compiler_error(
        &with(
            POINT_WITHOUT_HASH,
            r#"
fn main()
    println(f"{Point(x: 1, y: 2).hash()}")
"#,
        ),
        MISSING_HASH,
    );
}

#[test]
fn a_struct_holding_a_class_without_hash_is_refused_as_a_set_element() {
    assert_compiler_error(
        &with(
            POINT_WITHOUT_HASH,
            r#"
struct Tagged
    point Point
    tag int

fn main()
    let tagged = Set<Tagged>()
    tagged.add(Tagged(point: Point(x: 1, y: 2), tag: 1))
"#,
        ),
        MISSING_HASH,
    );
}

#[test]
fn a_generic_body_hashing_a_parameter_bound_to_a_class_without_hash_is_refused() {
    assert_compiler_error(
        &with(
            POINT_WITHOUT_HASH,
            r#"
fn hashed<T>(value T) int
    return value.hash()

fn main()
    println(f"{hashed(Point(x: 1, y: 2))}")
"#,
        ),
        "'hashed' hashes its 'T' parameter, so the type it is instantiated with has to have a hash",
    );
}

#[test]
fn a_set_of_lists_of_a_class_without_hash_is_refused() {
    assert_compiler_error(
        &with(
            POINT_WITHOUT_HASH,
            r#"
fn main()
    let lists = Set<List<Point>>()
    lists.add(List([Point(x: 1, y: 2)]))
"#,
        ),
        MISSING_HASH,
    );
}
