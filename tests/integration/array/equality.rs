// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `==` on arrays and lists compares their elements, in order, and sets and
//! maps holding them match by that comparison.

use super::utils::*;

/// Two arrays built separately with equal elements are equal; a different
/// element, or a different order, makes them unequal.
#[test]
fn arrays_compare_by_their_elements() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn main()
    let a = ["a", "b"]
    let b = ["a" + "", "b"]
    let c = ["b", "a"]
    println(f"{a == b} {a != b} {a == c} {a != c}")
"#,
        "true false false true",
    );
}

/// Lists compare element by element too, and lists of different lengths are
/// never equal.
#[test]
fn lists_compare_by_their_elements() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let a = List(["x", "y"])
    let b = List(["x" + "", "y"])
    var c = List(["x", "y"])
    c.push("z")
    let n = List([1, 2, 3])
    let m = List([1, 2, 3])
    println(f"{a == b} {a == c} {n == m}")
"#,
        "true false true",
    );
}

/// An element type `==` cannot compare leaves the collection uncomparable.
#[test]
fn a_list_of_functions_cannot_be_compared() {
    assert_compiler_error(
        r#"
use system.collections.list

fn adder(k int) fn(int) int
    return fn(n int) int: n + k

fn main()
    let a = List([adder(1)])
    let b = List([adder(1)])
    println(f"{a == b}")
"#,
        "MER_TYP_002",
    );
}

/// A set holding arrays keeps one of two arrays `==` calls equal, and a map
/// keyed by them has one key.
#[test]
fn sets_and_maps_match_arrays_by_their_elements() {
    assert_runs_with_output(
        r#"
use system.collections.array
use system.collections.set
use system.collections.map

fn main()
    var s = Set<Array<String, 2>>()
    s.add(["a", "b"])
    s.add(["a" + "", "b"])
    var m = Map<Array<String, 2>, int>()
    m[["a", "b"]] = 1
    m[["a" + "", "b"]] = 2
    let found = s.contains(["a", "b" + ""])
    println(f"{s.length()} {m.length()} {found}")
"#,
        "1 1 true",
    );
}
