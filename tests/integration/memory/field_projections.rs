// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for field-projection memory safety — the known Perceus pitfall where
// `Copy(obj.field)` does NOT receive an IncRef from the Perceus pass, yet the
// temp still gets a DecRef at StorageDead.  These tests exercise paths where
// collection/class fields are read, passed, iterated, and returned.

use super::super::utils::*;

#[test]
fn test_read_list_field_length_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Box
    var items [int]

fn main()
    let b = Box(items: List([1, 2, 3]))
    println(f"{b.items.length()}")
"#,
        "3",
    );
}

#[test]
fn test_read_map_field_length_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

class Store
    var data Map<String, int>

fn main()
    let s = Store(data: {"a": 1, "b": 2})
    println(f"{s.data.length()}")
"#,
        "2",
    );
}

#[test]
fn test_read_set_field_length_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

class Unique
    var vals {int}

fn main()
    let u = Unique(vals: {10, 20, 30})
    println(f"{u.vals.length()}")
"#,
        "3",
    );
}

#[test]
fn test_list_field_passed_to_function_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Container
    var data [int]

fn count(l [int]) int
    l.length()

fn main()
    let c = Container(data: List([1, 2, 3, 4]))
    println(f"{count(c.data)}")
    // c.data still valid after the call
    println(f"{c.data.length()}")
"#,
        "4\n4",
    );
}

#[test]
fn test_map_field_passed_to_function_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

class Registry
    var entries Map<String, int>

fn size(m Map<String, int>) int
    m.length()

fn main()
    let r = Registry(entries: {"x": 1, "y": 2, "z": 3})
    println(f"{size(r.entries)}")
    println(f"{r.entries.length()}")
"#,
        "3\n3",
    );
}

#[test]
fn test_for_loop_over_list_field_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Batch
    var values [int]

fn main()
    let b = Batch(values: List([10, 20, 30]))
    var total = 0
    for v in b.values
        total = total + v
    println(f"{total}")
"#,
        "60",
    );
}

#[test]
fn test_element_at_on_list_field_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Sequence
    var nums [int]

fn main()
    let s = Sequence(nums: List([5, 10, 15]))
    var i = 0
    var total = 0
    while i < s.nums.length()
        total = total + s.nums.element_at(i)
        i += 1
    println(f"{total}")
"#,
        "30",
    );
}

#[test]
fn test_nested_field_list_access_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Inner
    var data [int]

class Outer
    var inner Inner

fn main()
    let o = Outer(inner: Inner(data: List([100, 200, 300])))
    println(f"{o.inner.data.length()}")
    println(f"{o.inner.data.element_at(0)}")
"#,
        "3\n100",
    );
}

#[test]
fn test_deeply_nested_field_list_loop_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Level2
    var items [int]

class Level1
    var child Level2

class Root
    var level1 Level1

fn main()
    let r = Root(level1: Level1(child: Level2(items: List([1, 2, 3, 4, 5]))))
    var sum = 0
    for x in r.level1.child.items
        sum = sum + x
    println(f"{sum}")
"#,
        "15",
    );
}

#[test]
fn test_field_alias_outlives_parent_no_double_free() {
    // Take an alias of a list field; parent class goes out of scope;
    // the alias must keep the list alive.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Wrapper
    var data [int]

fn get_list() [int]
    let w = Wrapper(data: List([7, 8, 9]))
    let alias = w.data   // IncRef on data field
    alias                // w drops (DecRef data RC→1), alias returned (RC stays 1)

fn main()
    let l = get_list()
    println(f"{l.length()}")
"#,
        "3",
    );
}

#[test]
fn test_multiple_reads_of_same_field_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Box
    var items [int]

fn main()
    let b = Box(items: List([1, 2, 3]))
    let n1 = b.items.length()
    let n2 = b.items.length()
    let n3 = b.items.length()
    println(f"{n1 + n2 + n3}")
"#,
        "9",
    );
}
