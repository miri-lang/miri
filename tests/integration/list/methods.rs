// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn list_push_pop() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List<int>()
l.push(10)
l.push(20)
println(f\"{l.length()}\")
println(f\"{l.pop() ?? -1}\")
println(f\"{l.length()}\")
",
        "2\n20\n1",
    );
}

#[test]
fn list_insert_remove_at() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List([1, 3])
l.insert(1, 2)
println(f\"{l[0]} {l[1]} {l[2]}\")
println(f\"{l.remove_at(1) ?? -1}\")
println(f\"{l.length()}\")
",
        "1 2 3\n2\n2",
    );
}

#[test]
fn list_remove_by_value() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List([10, 20, 30])
println(f\"{l.remove(20)}\")
println(f\"{l.remove(99)}\")
println(f\"{l.length()}\")
println(f\"{l[1]}\")
",
        "true\nfalse\n2\n30",
    );
}

#[test]
fn list_clear() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List([1, 2, 3])
l.clear()
println(f\"{l.length()}\")
println(f\"{l.is_empty()}\")
",
        "0\ntrue",
    );
}

#[test]
fn list_reverse() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List([1, 2, 3])
l.reverse()
println(f\"{l[0]} {l[1]} {l[2]}\")
",
        "3 2 1",
    );
}

#[test]
fn list_baselist_queries() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List([10, 20, 30])
println(f\"{l.first() ?? -1}\")
println(f\"{l.last() ?? -1}\")
println(f\"{l.contains(20)}\")
println(f\"{l.index_of(30) ?? -1}\")
println(f\"{l.last_index() ?? -1}\")
",
        "10\n30\ntrue\n2\n2",
    );
}

#[test]
fn list_sort() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List([30, 10, 20, 5])
l.sort()
println(f\"{l[0]} {l[1]} {l[2]} {l[3]}\")
",
        "5 10 20 30",
    );
}

#[test]
fn list_get_method() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List([10, 20, 30])
println(f\"{l.get(0)}\")
println(f\"{l.get(2)}\")
",
        "10\n30",
    );
}

#[test]
fn list_set_method() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List([10, 20, 30])
l.set(1, 99)
println(f\"{l[0]} {l[1]} {l[2]}\")
",
        "10 99 30",
    );
}

#[test]
fn list_element_at() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List([10, 20, 30])
println(f\"{l.element_at(1)}\")
",
        "20",
    );
}

#[test]
fn list_remove_duplicate() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List([10, 20, 20, 30])
println(f\"{l.remove(20)}\")
println(f\"{l[0]} {l[1]} {l[2]}\")
println(f\"{l.length()}\")
",
        "true\n10 20 30\n3",
    );
}

/// `pop` reports an empty list as `None` rather than trapping, so draining a
/// list is ordinary control flow instead of a bounds precondition.
#[test]
fn list_pop_on_empty_is_none() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<int>()
    match l.pop()
        Some(v)
            println(f"unexpected {v}")
        None
            println("none")
"#,
        "none",
    );
}

/// An index past the end is an absence, not a failure: `remove_at` answers
/// `None` and leaves the list untouched.
#[test]
fn list_remove_at_out_of_bounds_is_none() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List([1, 2])
    match l.remove_at(5)
        Some(v)
            println(f"unexpected {v}")
        None
            println("none")
    println(f"{l.length()}")
"#,
        "none\n2",
    );
}

/// A negative index is out of range in the same way, and must not be
/// reinterpreted as an offset from the end.
#[test]
fn list_remove_at_negative_index_is_none() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List([1, 2])
    let i = 0 - 1
    match l.remove_at(i)
        Some(v)
            println(f"unexpected {v}")
        None
            println("none")
    println(f"{l.length()}")
"#,
        "none\n2",
    );
}

/// The positive side of both: a valid index and a non-empty list yield `Some`.
#[test]
fn list_pop_and_remove_at_yield_some() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List([10, 20, 30])
    match l.remove_at(0)
        Some(v)
            println(f"removed {v}")
        None
            println("unexpected none")
    match l.pop()
        Some(v)
            println(f"popped {v}")
        None
            println("unexpected none")
    println(f"{l.length()}")
"#,
        "removed 10\npopped 30\n1",
    );
}

/// The degenerate case of an out-of-range removal: an empty list has no valid
/// index at all, so even index 0 is an absence.
#[test]
fn list_remove_at_on_empty_is_none() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<int>()
    match l.remove_at(0)
        Some(v)
            println(f"unexpected {v}")
        None
            println("none")
    println(f"{l.length()}")
"#,
        "none\n0",
    );
}

/// Push with a projected field (struct field) should read the field's actual type,
/// not the struct's type, so RC accounting doesn't try to manage an int.
#[test]
fn list_push_projected_int_field() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Order
    amount int

fn main()
    let o = Order(100)
    var l = List<int>()
    l.push(o.amount)
    println(f"{l.length()}")
    println(f"{l[0]}")
"#,
        "1\n100",
    );
}

/// Push with a projected float field should also work without Cranelift verifier error.
#[test]
fn list_push_projected_float_field() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Price
    cost float

fn main()
    let p = Price(49.99)
    var l = List<float>()
    l.push(p.cost)
    println(f"{l.length()}")
    println(f"{l[0]}")
"#,
        "1\n49.99",
    );
}

/// Push with a projected bool field.
#[test]
fn list_push_projected_bool_field() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Flag
    enabled bool

fn main()
    let f = Flag(true)
    var l = List<bool>()
    l.push(f.enabled)
    println(f"{l.length()}")
    println(f"{l[0]}")
"#,
        "1\ntrue",
    );
}

/// Push with a managed (String) field should still work (regression guard).
#[test]
fn list_push_projected_string_field() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Item
    name String

fn main()
    let i = Item("widget")
    var l = List<String>()
    l.push(i.name)
    println(f"{l.length()}")
    println(f"{l[0]}")
"#,
        "1\nwidget",
    );
}

/// Insert with a projected int field.
#[test]
fn list_insert_projected_int_field() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Order
    amount int

fn main()
    let o = Order(100)
    var l = List<int>()
    l.insert(0, o.amount)
    println(f"{l.length()}")
    println(f"{l[0]}")
"#,
        "1\n100",
    );
}

/// Set (list.set) with a projected int field.
#[test]
fn list_set_projected_int_field() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Order
    amount int

fn main()
    let o = Order(100)
    var l = List([0, 0])
    l.set(0, o.amount)
    println(f"{l[0]}")
    println(f"{l[1]}")
"#,
        "100\n0",
    );
}

/// Push with a call-result temp that has a projected field.
#[test]
fn list_push_projected_from_call_result() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Order
    amount int

fn make_order() Order
    Order(100)

fn main()
    var l = List<int>()
    l.push(make_order().amount)
    println(f"{l.length()}")
    println(f"{l[0]}")
"#,
        "1\n100",
    );
}
