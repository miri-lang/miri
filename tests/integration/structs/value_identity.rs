// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A value compares as `==` says wherever it is compared: a member of a
//! counted aggregate is compared member by member, and a set or map holding
//! tuples matches them member by member — whether a tuple is held as its
//! bytes, with a float member or padding between members, or is a counted
//! allocation holding a string, an optional or a struct.

use super::utils::*;

#[test]
fn test_a_value_inside_a_counted_tuple_or_struct_is_compared_by_its_members() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

struct P
    x float
    y float

struct Named
    name String
    at P

fn mt(y float) (String, P)
    return ("a", P(x: 1.0, y: y))

fn named(y float) Named: Named(name: "a", at: P(x: 1.0, y: y))

fn main()
    let xs = List([mt(2.0)])
    let zero = mt(0.0)
    let negative = mt(-0.0)
    println(f"{mt(2.0) == mt(3.0)} {mt(2.0) == mt(2.0)} {xs.contains(mt(3.0))} {zero == negative} {named(2.0) == named(3.0)} {named(1.0) == named(1.0)}")
"#,
        "false true false true false true",
    );
}

#[test]
fn test_a_set_of_float_tuples_matches_negative_and_positive_zero() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

fn main()
    var s = Set<(float, float)>()
    s.add((-0.0, 1.0))
    s.add((0.0, 1.0))
    let zero = 0.0
    let nan = zero / zero
    var n = Set<(float, int)>()
    n.add((nan, 1))
    n.add((nan, 1))
    println(f"{s.length()} {s.contains((0.0, 1.0))} {n.length()}")
"#,
        "1 true 2",
    );
}

#[test]
fn test_a_set_of_padded_tuples_ignores_the_padding_bytes() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

fn dirty() int
    let junk = (-1, -1, -1, -1)
    junk.0 + junk.2

fn key(a i8, b int) (i8, int)
    let d = dirty()
    (a, b + d - d)

fn main()
    var s = Set<(i8, int)>()
    let k0 = (1 as i8, 2)
    s.add(k0)
    s.add(key(1, 2))
    println(f"{s.length()} {s.contains(key(1, 2))}")
"#,
        "1 true",
    );
}

#[test]
fn test_a_map_keyed_by_tuples_matches_keys_by_their_members() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

fn main()
    var m = Map<(float, int), String>()
    m[(-0.0, 3)] = "neg"
    m[(0.0, 3)] = "pos"
    var p = Map<(i8, int), int>()
    let one i8 = 1
    p[(one, 2)] = 5
    let found = p.contains_key((one, 2))
    println(f"{m.length()} {m[(0.0, 3)]} {found}")
"#,
        "1 pos true",
    );
}

#[test]
fn test_a_copied_padded_struct_compares_and_hashes_as_its_source() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

struct Key
    tag i8
    id int

fn dirty() int
    let junk = (-1, -1, -1, -1)
    junk.0 + junk.2

fn key(tag i8, id int) Key
    let d = dirty()
    Key(tag: tag, id: id + d - d)

fn main()
    let k = key(1, 2)
    var c = k
    let maybe Key? = c
    var s = Set<Key>()
    s.add(k)
    s.add(c)
    println(f"{c == k} {c == key(1, 3)} {s.length()} {s.contains(key(1, 2))} {maybe == k}")
"#,
        "true false 1 true true",
    );
}

#[test]
fn test_a_set_of_tuples_holding_strings_matches_them_by_their_members() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

fn word(n int) String: f"w{n}"

fn main()
    var s = Set<(int, String)>()
    s.add((1, "a"))
    s.add((1, "a"))
    s.add((1, word(2)))
    s.add((1, word(2)))
    s.add((2, "a"))
    println(f"{s.length()} {s.contains((1, word(2)))} {s.contains((3, word(2)))}")
"#,
        "3 true false",
    );
}

#[test]
fn test_a_map_keyed_by_tuples_holding_strings_finds_an_equal_key() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

fn word(n int) String: f"w{n}"

fn main()
    var m = Map<(String, int), int>()
    m[(word(1), 7)] = 1
    m[(word(1), 7)] = 2
    m[("w1", 8)] = 3
    let other = ("w1", 8)
    println(f"{m.length()} {m[(word(1), 7)]} {m.contains_key(other)}")
"#,
        "2 2 true",
    );
}

#[test]
fn test_a_set_of_tuples_holding_optional_strings_matches_them_by_their_members() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

fn word(n int) String?: Some(f"w{n}")

fn main()
    var s = Set<(int, String?)>()
    s.add((1, word(1)))
    s.add((1, word(1)))
    s.add((1, None))
    s.add((1, None))
    println(f"{s.length()} {s.contains((1, word(1)))}")
"#,
        "2 true",
    );
}

#[test]
fn test_a_set_of_tuples_holding_strings_finds_removes_and_clones_by_members() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

fn word(n int) String: f"w{n}"

fn main()
    var s = Set<(int, String)>()
    s.add((1, word(1)))
    s.add((1, word(1)))
    s.add((2, word(2)))
    let had = s.contains((1, word(1)))
    s.remove((1, word(1)))
    s.remove((5, word(5)))
    let c = s.clone()
    s.add((3, word(3)))
    println(f"{had} {s.length()} {s.contains((1, word(1)))} {c.length()} {c.contains((2, word(2)))} {c.contains((3, word(3)))}")
"#,
        "true 2 false 1 true false",
    );
}

#[test]
fn test_a_map_keyed_by_tuples_holding_strings_removes_an_equal_key() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

fn word(n int) String: f"w{n}"

fn main()
    var m = Map<(String, int), int>()
    m[(word(1), 1)] = 10
    m[(word(1), 1)] = 11
    m[(word(2), 2)] = 20
    let g = m[(word(1), 1)]
    m.remove((word(1), 1))
    m.remove((word(9), 9))
    println(f"{g} {m.length()} {m.contains_key((word(1), 1))} {m[(word(2), 2)]}")
"#,
        "11 1 false 20",
    );
}

#[test]
fn test_a_set_of_nested_tuples_matches_the_inner_tuples_by_their_members() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

fn word(n int) String: f"w{n}"

fn main()
    var s = Set<((int, String), String)>()
    s.add(((1, word(1)), word(2)))
    s.add(((1, word(1)), word(2)))
    s.add(((1, word(2)), word(2)))
    println(f"{s.length()} {s.contains(((1, word(1)), word(2)))} {s.contains(((2, word(1)), word(2)))}")
"#,
        "2 true false",
    );
}

#[test]
fn test_a_set_of_tuples_holding_a_struct_matches_the_struct_by_its_fields() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

struct Named
    name String
    id int

fn named(n int) Named: Named(name: f"w{n}", id: n)

fn main()
    var s = Set<(Named, int)>()
    s.add((named(1), 1))
    s.add((named(1), 1))
    s.add((named(2), 1))
    s.remove((named(2), 1))
    println(f"{s.length()} {s.contains((named(1), 1))} {s.contains((named(2), 1))}")
"#,
        "1 true false",
    );
}

#[test]
fn test_a_generic_set_of_tuples_matches_them_at_its_instantiation() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

fn word(n int) String: f"w{n}"

fn pairs<T>(a T, b T) Set<(T, String)>
    var s = Set<(T, String)>()
    s.add((a, word(1)))
    s.add((b, word(1)))
    s

fn main()
    let s = pairs(1, 1)
    let t = pairs(1, 2)
    println(f"{s.length()} {t.length()} {s.contains((1, word(1)))}")
"#,
        "1 2 true",
    );
}

#[test]
fn test_sets_of_tuples_at_different_value_generic_instantiations_stay_apart() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

class Tag<T, Size>
    var v int
    fn init(v int)
        self.v = v
    fn equals(_other Tag<T, Size>) bool
        return Size == 3
    fn hash() int
        return 0

fn main()
    var a = Set<(Tag<int, 2>, String)>()
    a.add((Tag<int, 2>(1), "x"))
    a.add((Tag<int, 2>(1), "x"))
    var b = Set<(Tag<int, 3>, String)>()
    b.add((Tag<int, 3>(1), "x"))
    b.add((Tag<int, 3>(1), "x"))
    println(f"{a.length()} {b.length()}")
"#,
        "2 1",
    );
}

#[test]
fn test_sets_and_maps_of_tuples_differing_only_in_shape_each_match_their_own() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set
use system.collections.map

fn word(n int) String: f"w{n}"

fn main()
    var a = Set<(int, String)>()
    a.add((1, word(1)))
    a.add((1, word(1)))
    a.add((2, word(1)))
    var b = Set<(String, int)>()
    b.add((word(1), 1))
    b.add((word(1), 1))
    var c = Set<((int, String), String)>()
    c.add(((1, word(1)), word(2)))
    c.add(((1, word(1)), word(2)))
    c.add(((1, word(1)), word(3)))
    c.add(((2, word(1)), word(3)))
    var m = Map<(int, (int, String)), int>()
    m[(1, (2, word(3)))] = 1
    m[(1, (2, word(3)))] = 2
    m[(1, (2, word(4)))] = 3
    println(f"{a.length()} {b.length()} {c.length()} {m.length()} {m[(1, (2, word(3)))]}")
"#,
        "2 1 3 2 2",
    );
}

#[test]
fn test_a_set_of_float_and_string_tuples_matches_zeroes_and_keeps_nans_apart() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

fn word(n int) String: f"w{n}"

fn main()
    let zero = 0.0
    let nan = zero / zero
    var s = Set<(float, String)>()
    s.add((0.0, word(1)))
    s.add((-0.0, word(1)))
    s.add((nan, word(1)))
    s.add((nan, word(1)))
    println(f"{s.length()} {s.contains((-0.0, word(1)))}")
"#,
        "3 true",
    );
}

#[test]
fn test_a_set_of_tuples_iterates_and_answers_in() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

fn word(n int) String: f"w{n}"

fn main()
    var s = Set<(int, String)>()
    s.add((1, word(1)))
    s.add((2, word(2)))
    s.add((1, word(1)))
    var total = 0
    var letters = 0
    for e in s
        total = total + e.0
        letters = letters + e.1.length()
    println(f"{total} {letters} {(2, word(2)) in s} {(3, word(2)) in s}")
"#,
        "3 4 true false",
    );
}

#[test]
fn test_a_cloned_map_keyed_by_tuples_finds_an_equal_key() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

fn word(n int) String: f"w{n}"

fn main()
    var m = Map<(int, String), (String, int)>()
    m[(1, word(1))] = (word(10), 10)
    m[(1, word(1))] = (word(11), 11)
    let c = m.clone()
    m[(2, word(2))] = (word(20), 20)
    let v = c[(1, word(1))]
    println(f"{c.length()} {c.contains_key((1, word(1)))} {c.contains_key((2, word(2)))} {v.0} {v.1} {m.length()}")
"#,
        "1 true false w11 11 2",
    );
}

#[test]
fn test_a_set_of_tuples_filters_maps_and_reduces() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

fn word(n int) String: f"w{n}"

fn main()
    var s = Set<(int, String)>()
    for i in 0..4
        s.add((i, word(i)))
    let kept = s.filter(fn(e (int, String)) bool: e.0 % 2 == 0)
    let paired = s.map(fn(e (int, String)) (int, String): (e.0 % 2, word(e.0 % 2)))
    let total = s.reduce((0, ""), fn(acc (int, String), e (int, String)) (int, String): (acc.0 + e.0, acc.1 + e.1))
    println(f"{kept.length()} {paired.length()} {total.0} {total.1.length()}")
"#,
        "2 2 6 8",
    );
}

#[test]
fn test_a_set_of_tuples_keeps_one_of_each_while_it_grows() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

fn word(n int) String: f"w{n}"

fn main()
    var s = Set<(int, String)>()
    for i in 0..200
        s.add((i % 50, word(i % 50)))
    println(f"{s.length()} {s.contains((49, word(49)))} {s.contains((50, word(50)))}")
"#,
        "50 true false",
    );
}

/// A tuple member whose `equals` replaces the field holding the set being
/// searched. The tuple's equality reads its later string member after that
/// call returns, which must still find both tuples alive.
#[test]
fn test_a_tuple_member_equals_that_replaces_the_searched_set_leaves_the_tuples_alive() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

class Holder
    var s {(Elem, String)}
    var armed bool

    fn replace()
        if self.armed
            self.armed = false
            self.s = Set<(Elem, String)>()

class Elem implements Equatable, Hashable
    var h Holder?
    var k int

    fn poke()
        match self.h
            Some(h): h.replace()
            None: return

    fn equals(other Elem) bool
        self.poke()
        other.poke()
        return self.k == other.k

    fn hash() int
        return 0

fn main()
    var h = Holder(s: Set<(Elem, String)>(), armed: false)
    for i in 0..8
        h.s.add((Elem(h: None, k: i), f"w{i}"))
    h.armed = true
    let found = h.s.contains((Elem(h: Some(h), k: 3), "w3"))
    println(f"{found} {h.s.length()}")
"#,
        "true 0",
    );
}

/// Two tuple types differing only in the signature of a function value they
/// reach, through a class matched by identity, name two equality bodies, not
/// one shared by both.
#[test]
fn test_sets_of_tuples_holding_functions_of_different_signatures_stay_apart() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

class Holder<F>
    var f F

fn word(n int) String: f"w{n}"

fn main()
    let a = Holder<fn(int) int>(f: fn(x int) int: x + 1)
    let b = Holder<fn(String) int>(f: fn(x String) int: 1)
    var s = Set<(Holder<fn(int) int>, String)>()
    s.add((a, word(1)))
    s.add((a, word(1)))
    var t = Set<(Holder<fn(String) int>, String)>()
    t.add((b, word(1)))
    t.add((b, word(2)))
    println(f"{s.length()} {t.length()}")
"#,
        "1 2",
    );
}
