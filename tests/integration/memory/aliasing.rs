// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for RC aliasing: verifies that sharing a managed object between multiple
// variables never causes double-free or leaks regardless of which variable drops last.

use super::super::utils::*;

#[test]
fn test_list_three_way_alias_no_leak() {
    // Three variables sharing a single List; each DecRef reduces RC by one.
    assert_runs(
        r#"
use system.collections.list

let l1 = List([1, 2, 3])
let l2 = l1
let l3 = l2
// l3 drops last, RC should reach 0 exactly once
"#,
    );
}

#[test]
fn test_list_alias_then_original_dropped_via_reassign() {
    // Alias stays alive while the original is overwritten — the underlying
    // buffer must survive until the alias is also dropped.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l1 = List([10, 20, 30])
    let l2 = l1          // RC = 2
    l1 = List([99])      // DecRef old → RC = 1; new list RC = 1

    println(f"{l2.length()}")
    println(f"{l1.length()}")
"#,
        "3\n1",
    );
}

#[test]
fn test_list_alias_mutation_cow_isolates() {
    // CoW: a.push(3) clones a's data (RC=3 → new ptr for a); b and c retain the original.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let a = List([1, 2])
    let b = a
    let c = b
    var a2 = a
    a2.push(3)

    println(f"{c.length()}")
"#,
        "2",
    );
}

#[test]
fn test_list_alias_in_nested_scope() {
    // Alias created in an inner scope; must be dropped at end of that scope,
    // leaving the outer binding intact (no use-after-free).
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let outer = List([1, 2, 3])
    var i = 0
    while i < 3
        let scoped = outer  // IncRef each iteration
        i += 1
        // scoped drops here (DecRef)
    println(f"{outer.length()}")
"#,
        "3",
    );
}

#[test]
fn test_map_three_way_alias() {
    assert_runs(
        r#"
use system.collections.map
let m1 = {"x": 1, "y": 2}
let m2 = m1
let m3 = m2
"#,
    );
}

#[test]
fn test_map_alias_survives_original_reassign() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m1 = {"a": 10}
    let m2 = m1
    m1 = {"b": 20}
    println(f"{m2.length()}")
    println(f"{m1.length()}")
"#,
        "1\n1",
    );
}

#[test]
fn test_set_three_way_alias() {
    // Three variables sharing a single Set; the last drop must free it exactly
    // once, and every alias must still see all three elements.
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let s1 = {10, 20, 30}
    let s2 = s1
    let s3 = s1
    println(f"s2={s2.length()} s3={s3.length()}")
"#,
        "s2=3 s3=3",
    );
}

#[test]
fn test_set_alias_mutation_cow_isolates() {
    // CoW: s1.add(4) clones s1's data; s2 retains original 3 elements.
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let s1 = {1, 2, 3}
    let s2 = s1
    var s3 = s1
    s3.add(4)
    println(f"{s2.length()}")
"#,
        "3",
    );
}

#[test]
fn test_class_three_way_alias() {
    assert_runs_with_output(
        r#"

class Node
    var value int

fn main()
    let n1 = Node(value: 42)
    let n2 = n1
    let n3 = n2
    println(f"{n3.value}")
"#,
        "42",
    );
}

#[test]
fn test_class_alias_mutation_visible() {
    // Mutation on one alias is visible through another.
    assert_runs_with_output(
        r#"

class Counter
    var count int

fn main()
    var c1 = Counter(count: 0)
    let c2 = c1
    c1.count = 5
    println(f"{c2.count}")
"#,
        "5",
    );
}

#[test]
fn test_class_alias_in_function_scope_no_leak() {
    // Alias passed into a function; function's copy goes out of scope first.
    assert_runs_with_output(
        r#"

class Box
    var val int

fn inspect(b Box) int
    b.val

fn main()
    let b = Box(val: 99)
    let alias = b
    let v = inspect(b)
    println(f"{v}")
    println(f"{alias.val}")
"#,
        "99\n99",
    );
}

#[test]
fn test_multiple_managed_types_aliased_simultaneously() {
    // Several different managed types alive at once; all must be freed exactly once.
    assert_runs_with_output(
        r#"
use system.collections.list
use system.collections.map
use system.collections.set

fn main()
    let l1 = List([1, 2, 3])
    let l2 = l1
    let m1 = {"a": 10}
    let m2 = m1
    let s1 = {100, 200}
    let s2 = s1
    println(f"{l2.length()}")
    println(f"{m2.length()}")
    println(f"{s2.length()}")
"#,
        "3\n1\n2",
    );
}

/// A callee that replaces a field of an object while it still reads a list
/// passed to it from that same field keeps reading the list it was given.
#[test]
fn test_parameter_survives_its_owner_replacing_the_field_it_came_from() {
    assert_runs_with_output(
        r#"
use system.collections.list

class H
    var items [int]

    fn init(items [int])
        self.items = items

    fn replace()
        self.items = List([9, 9, 9, 9])

fn total(l [int], hh H) int
    var s = 0
    for i in 0..l.length()
        hh.replace()
        s = s + l[i]
    s

fn main()
    let h = H(List([1, 2, 3, 4, 5, 6, 7, 8]))
    println(f"{total(h.items, h)}")
    println(f"{h.items.length()}")
"#,
        "36\n4",
    );
}

/// An object passed beside one of its own fields: storing a new value into
/// that field through the object leaves the field argument readable.
#[test]
fn test_field_argument_survives_a_store_into_its_object() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Cn
    var items [int]

    fn init(items [int])
        self.items = items

fn sum_after_store(w Cn, extra [int]) int
    var x = w
    x.items = List([0, 0, 0])
    return extra[0] + extra[1] + extra[2]

fn main()
    var c = Cn(List([1, 2, 3]))
    println(f"{sum_after_store(c, c.items)} {c.items[0]}")
"#,
        "6 0",
    );
}

/// A scalar-only struct read out of a list or array is a copy: writing the
/// copy leaves the element alone.
#[test]
fn test_struct_read_out_of_a_collection_is_a_copy() {
    assert_runs_with_output(
        r#"
use system.collections.list
use system.collections.array

struct P
    count int
    weight int

fn main()
    var a = List([P(count: 1, weight: 2), P(count: 3, weight: 4)])
    var p = a[0]
    p.count = 150
    var q = a[0]
    q = a[1]
    q.weight = 99
    var arr = [P(count: 5, weight: 6)]
    var r = arr[0]
    r.count = 7
    println(f"{a[0].count} {p.count} {a[1].weight} {q.weight} {arr[0].count} {r.count}")
"#,
        "1 150 4 99 5 7",
    );
}
