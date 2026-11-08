// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How a map decides that two keys are the same key.

use super::utils::*;

#[test]
fn map_with_optional_keys_matches_by_content() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<int?, String>()
    m.set(Some(2), "two")
    let two = m.get(Some(2)) ?? "none"
    let has = m.contains_key(Some(2))
    println(f"{m.length()} {two} {has}")
"#,
        "1 two true",
    );
}

#[test]
fn set_of_optionals_deduplicates_by_content() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<int?>()
    s.add(Some(2))
    s.add(Some(2))
    let has = s.contains(Some(2))
    println(f"{s.length()} {has}")
"#,
        "1 true",
    );
}

#[test]
fn set_of_optionals_treats_none_as_one_value() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<int?>()
    s.add(None)
    s.add(None)
    let has = s.contains(None)
    println(f"{s.length()} {has}")
"#,
        "1 true",
    );
}

#[test]
fn equality_on_optionals_is_already_correct() {
    // The rule a container would have to apply exists and works when written by
    // hand. This is what makes the container case a matter of reaching those
    // semantics rather than deciding them.
    assert_runs_with_output(
        r#"
fn main()
    let a int? = Some(2)
    let b int? = Some(2)
    let c int? = None
    let d int? = None
    let e int? = Some(3)
    println(f"{a == b} {c == d} {a == e} {a == c}")
"#,
        "true true false false",
    );
}

#[test]
fn map_with_optional_string_keys_matches_by_content() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String?, int>()
    m.set(Some("two"), 2)
    let n = m.get(Some("two")) ?? 0
    let has = m.contains_key(Some("two"))
    println(f"{m.length()} {n} {has}")
"#,
        "1 2 true",
    );
}

#[test]
fn map_overwrites_the_entry_an_equal_optional_key_already_holds() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<int?, String>()
    m.set(Some(2), "two")
    m.set(Some(2), "TWO")
    let v = m.get(Some(2)) ?? "none"
    println(f"{m.length()} {v}")
"#,
        "1 TWO",
    );
}

#[test]
fn map_literal_with_optional_keys_matches_by_content() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = {Some(5): "five", Some(6): "six"}
    let v = m.get(Some(5)) ?? "none"
    println(f"{m.length()} {v}")
"#,
        "2 five",
    );
}

#[test]
fn map_index_write_reaches_the_entry_an_equal_optional_key_holds() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<int?, String>()
    m[Some(3)] = "three"
    m[Some(3)] = "THREE"
    let v = m.get(Some(3)) ?? "none"
    println(f"{m.length()} {v}")
"#,
        "1 THREE",
    );
}

#[test]
fn a_none_key_is_a_key_of_its_own_beside_a_some_key() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<int?, String>()
    m.set(None, "nothing")
    m.set(Some(0), "zero")
    let absent = m.get(None) ?? "?"
    let zero = m.get(Some(0)) ?? "?"
    println(f"{m.length()} {absent} {zero}")
"#,
        "2 nothing zero",
    );
}

#[test]
fn set_of_optional_strings_deduplicates_by_content() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<String?>()
    s.add(Some("a"))
    s.add(Some("a"))
    s.add(None)
    let has_a = s.contains(Some("a"))
    let has_none = s.contains(None)
    let has_b = s.contains(Some("b"))
    println(f"{s.length()} {has_a} {has_none} {has_b}")
"#,
        "2 true true false",
    );
}

#[test]
fn a_set_of_optionals_distinguishes_different_payloads() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<int?>()
    s.add(Some(1))
    s.add(Some(2))
    s.add(Some(1))
    let has_one = s.contains(Some(1))
    let has_three = s.contains(Some(3))
    println(f"{s.length()} {has_one} {has_three}")
"#,
        "2 true false",
    );
}

/// The value inside a `Some` box is read at its own width, not the box's. An
/// `i32?` box is a pointer wide because every boxed payload is, so the four
/// bytes past the value are whatever the allocator left there; reading them
/// would make two equal keys differ by uninitialized memory.
#[test]
fn a_payload_narrower_than_its_box_is_matched_on_its_own_bytes() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<i32?>()
    var i i32 = 0
    while i < 40
        s.add(Some(7))
        s.add(Some(i))
        i = i + 1
    let seven = s.contains(Some(7))
    let last = s.contains(Some(39))
    let absent = s.contains(Some(100))
    println(f"{s.length()} {seven} {last} {absent}")
"#,
        "40 true true false",
    );
}

/// Every optional wrapping the element is opened, so a `None` is the same value
/// only as a `None` found at the same depth: `Some(None)` is neither `None` nor
/// `Some(Some(x))`.
#[test]
fn nested_optionals_are_matched_at_every_level() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<Option<Option<int>>>()
    s.add(Some(Some(2)))
    s.add(Some(Some(2)))
    s.add(Some(None))
    s.add(Some(None))
    s.add(None)
    s.add(None)
    let two = s.contains(Some(Some(2)))
    let inner_none = s.contains(Some(None))
    let outer_none = s.contains(None)
    let three = s.contains(Some(Some(3)))
    println(f"{s.length()} {two} {inner_none} {outer_none} {three}")
"#,
        "3 true true true false",
    );
}

/// A wrapped value whose type answers `equals` is asked through it, the same
/// way the unwrapped element would be.
#[test]
fn an_optional_class_element_is_matched_through_its_own_equals() {
    assert_runs_with_output(
        r#"
use system.collections.set

class Tag
    let name String
    fn init(name String)
        self.name = name
    fn equals(other Tag) bool
        return self.name == other.name
    fn hash() int
        return self.name.hash()

fn main()
    var s = Set<Tag?>()
    s.add(Some(Tag("a")))
    s.add(Some(Tag("a")))
    s.add(Some(Tag("b")))
    s.add(None)
    let a = s.contains(Some(Tag("a")))
    let z = s.contains(Some(Tag("z")))
    println(f"{s.length()} {a} {z}")
"#,
        "3 true false",
    );
}

/// A struct element is matched by its fields, as `==` compares it — for the
/// wrapped element exactly as for the bare one.
#[test]
fn an_optional_struct_element_is_matched_by_its_fields() {
    assert_runs_with_output(
        r#"
use system.collections.set

struct Point
    x int
    y int

fn main()
    var s = Set<Point?>()
    s.add(Some(Point(1, 2)))
    s.add(Some(Point(1, 2)))
    var bare = Set<Point>()
    bare.add(Point(1, 2))
    bare.add(Point(1, 2))
    println(f"{s.length()} {bare.length()}")
"#,
        "1 1",
    );
}

/// Removing an entry an optional key holds reaches it, and the keys left behind
/// stay reachable across the rehash a growing map performs.
#[test]
fn optional_keys_survive_removal_and_growth() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<int?, int>()
    var i = 0
    while i < 60
        m.set(Some(i), i * 2)
        i = i + 1
    m.remove(Some(37))
    let gone = m.get(Some(37)) ?? -1
    let kept = m.get(Some(38)) ?? -1
    println(f"{m.length()} {gone} {kept}")
"#,
        "59 -1 76",
    );
}

/// A map matches its keys by `==`, which cannot compare two functions, so a
/// function type is refused as a key; as a value it stays allowed.
#[test]
fn a_function_is_refused_as_a_map_key_but_not_as_a_value() {
    assert_compiler_error(
        r#"
use system.collections.map

fn adder(k int) fn(int) int
    return fn(n int) int: n + k

fn main()
    var m = Map<fn(int) int, int>()
    m[adder(10)] = 1
"#,
        "cannot be a Map key, which is matched by `==`",
    );
    assert_runs_with_output(
        r#"
use system.collections.map

fn adder(k int) fn(int) int
    return fn(n int) int: n + k

fn main()
    var m = Map<int, fn(int) int>()
    m[1] = adder(10)
    let f = m[1]
    println(f"{f(1)}")
"#,
        "11",
    );
}

/// A tuple holding a function is no more comparable than the function is, so
/// it is refused as a key.
#[test]
fn a_tuple_holding_a_function_is_refused_as_a_map_key() {
    let program = r#"
use system.collections.map

fn adder(k int) fn(int) int
    return fn(n int) int: n + k

fn main()
    var m = Map<(int, fn(int) int), int>()
    m[(1, adder(10))] = 1
"#;
    assert_compiler_error(program, "MER_TYP_002");
    assert_compiler_error(program, "cannot be a Map key, which is matched by `==`");
}

/// A map holding function values is a type holding one, so a tuple holding
/// such a map is refused as a key.
#[test]
fn a_tuple_holding_a_map_of_functions_is_refused_as_a_map_key() {
    let program = r#"
use system.collections.map

fn main()
    let f = fn(x int) int: x + 1
    var fm = Map<int, fn(int) int>()
    fm[1] = f
    var m = Map<(int, Map<int, fn(int) int>), int>()
    m[(1, fm)] = 1
"#;
    assert_compiler_error(program, "MER_TYP_002");
    assert_compiler_error(program, "cannot be a Map key, which is matched by `==`");
}

/// A set key is matched through the set's own `equals`: a set holding the
/// same elements finds the entry, and setting it again overwrites the value.
#[test]
fn map_with_set_keys_matches_by_their_elements() {
    assert_runs_with_output(
        r#"
use system.collections.map
use system.collections.set

fn main()
    var m = Map<Set<String>, int>()
    m.set({"a", "b"}, 1)
    m.set({"b", "a"}, 2)
    let found = m.get({"b", "a"}) ?? 0
    let missing = m.get({"a"}) ?? 0
    let has = m.contains_key({"a", "b"})
    println(f"{m.length()} {found} {missing} {has}")
"#,
        "1 2 0 true",
    );
}

#[test]
fn map_with_map_keys_matches_by_their_entries() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<Map<String, int>, String>()
    m.set({"x": 1, "y": 2}, "first")
    m.set({"y": 2, "x": 1}, "second")
    m.set({"x": 1}, "other")
    let found = m.get({"y": 2, "x": 1}) ?? "none"
    println(f"{m.length()} {found}")
"#,
        "2 second",
    );
}

#[test]
fn maps_keyed_by_maps_and_sets_release_what_they_hold() {
    assert_heap_guard_output(
        r#"
use system.collections.map
use system.collections.set

fn main()
    var by_map = Map<Map<String, int>, String>()
    var by_set = Map<Set<int>, int>()
    var total = 0
    for i in 0..20
        by_map.set({f"k{i % 3}": i % 3}, f"v{i}")
        by_set.set({i % 5, 100}, i)
        total = total + (by_set.get({100, i % 5}) ?? 0)
        let seen = by_map.get({f"k{i % 3}": i % 3}) ?? ""
        if seen.length() == 0
            total = total - 1000
    println(f"{by_map.length()} {by_set.length()} {total}")
"#,
        "3 5 190",
    );
}
