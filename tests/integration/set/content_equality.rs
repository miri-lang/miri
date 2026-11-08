// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `==` and `!=` on sets compare contents: two sets are equal when they hold
//! the same elements, whatever order they went in. Two sets built separately
//! are never the same object, so comparing them by identity would call every
//! pair unequal.

use super::utils::*;

#[test]
fn sets_with_equal_elements_in_a_different_order_are_equal() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let a = {1, 2, 3}
    let b = {3, 1, 2}
    println(f"{a == b} {a != b} {b == a}")
"#,
        "true false true",
    );
}

#[test]
fn sets_differing_in_size_or_element_are_unequal() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let base = {1, 2}
    let longer = {1, 2, 3}
    let other = {1, 3}
    println(f"{base == longer} {longer == base} {base == other} {base != other}")
"#,
        "false false false true",
    );
}

#[test]
fn empty_sets_are_equal_and_a_set_equals_itself() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let e1 = Set<int>()
    let e2 = Set<int>()
    let full = {7}
    println(f"{e1 == e2} {e1 == full} {full == full} {full != full}")
"#,
        "true false true false",
    );
}

#[test]
fn sets_of_strings_compare_by_content() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let a = {"a" + "b", "cd"}
    let b = {"cd", "ab"}
    let c = {"ab", "ce"}
    println(f"{a == b} {a == c}")
"#,
        "true false",
    );
}

#[test]
fn sets_of_tuples_compare_by_content() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var a = Set<(int, String)>()
    a.add((1, "one"))
    a.add((2, "two"))
    var b = Set<(int, String)>()
    b.add((2, "two"))
    b.add((1, "one"))
    var c = Set<(int, String)>()
    c.add((1, "one"))
    c.add((2, "TWO"))
    println(f"{a == b} {a == c}")
"#,
        "true false",
    );
}

#[test]
fn sets_equal_by_content_hash_alike_whatever_their_order() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let a = {"a", "b", "c"}
    let b = {"c", "a", "b"}
    let c = {"a", "b", "d"}
    println(f"{a.hash() == b.hash()} {a.hash() == c.hash()}")
"#,
        "true false",
    );
}

#[test]
fn comparing_sets_of_strings_in_a_loop_releases_what_it_reads() {
    assert_heap_guard_output(
        r#"
use system.collections.set

fn main()
    var equal = 0
    var i = 0
    while i < 50
        let a = {"a" + "b", "c"}
        let b = {"c", "ab"}
        let c = {"ab", "d"}
        if a == b: equal += 1
        if a != c: equal += 1
        i += 1
    println(f"{equal}")
"#,
        "100",
    );
}

#[test]
fn a_set_of_class_instances_without_equals_never_compared_still_runs() {
    assert_runs_with_output(
        r#"
use system.collections.set

class Foo
    var x int

fn main()
    let f = Foo(x: 1)
    var s = Set<Foo>()
    s.add(f)
    s.add(f)
    s.add(Foo(x: 1))
    println(f"{s.length()}")
"#,
        "2",
    );
}
