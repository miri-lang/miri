// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A value held as its bytes compares as `==` says wherever it is compared:
//! a member of a counted aggregate is compared member by member, and a set or
//! map holding tuples matches them field by field whenever their bytes are
//! not their value — a float member, or padding between members.

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
