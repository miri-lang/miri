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
