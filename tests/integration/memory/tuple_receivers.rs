// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A tuple is a value: writing a collection held in one of its elements writes
//! a collection only that tuple holds. A tuple holding a managed value is a
//! reference-counted block that a second binding shares, so the write first
//! copies the tuple when another binding holds it, then the collection when
//! another holder shares it, and leaves every other holder with what it had.

use super::super::utils::*;

/// The program that runs `body` inside `main`, importing the list module.
fn list_program(body: &str) -> String {
    format!("use system.io\nuse system.collections.list\n\nfn main()\n{body}")
}

#[test]
fn test_resizing_a_list_in_a_tuple_leaves_the_list_it_was_built_from() {
    assert_runs_with_output(
        &list_program(
            "    var a = List([1, 2, 3])\n    var t = (a, 1)\n    t.0.resize(6, 9)\n    println(f\"{a.length()} {t.0.length()} {t.0[5]}\")\n",
        ),
        "3 6 9",
    );
}

#[test]
fn test_pushing_onto_a_list_in_a_tuple_leaves_the_list_it_was_built_from() {
    assert_runs_with_output(
        &list_program(
            "    var a = List([1, 2, 3])\n    var t = (a, 1)\n    t.0.push(9)\n    println(f\"{a.length()} {t.0.length()} {t.0[3]}\")\n",
        ),
        "3 4 9",
    );
}

#[test]
fn test_clearing_a_list_in_a_tuple_leaves_the_list_it_was_built_from() {
    assert_runs_with_output(
        &list_program(
            "    var a = List([1, 2, 3])\n    var t = (a, 1)\n    t.0.clear()\n    println(f\"{a.length()} {t.0.length()}\")\n",
        ),
        "3 0",
    );
}

#[test]
fn test_a_write_through_a_shared_tuple_leaves_the_other_binding_alone() {
    assert_runs_with_output(
        &list_program(
            "    var a = List([1, 2, 3])\n    var t = (a, 1)\n    var u = t\n    t.0.push(9)\n    u.0.resize(6, 9)\n    println(f\"{a.length()} {t.0.length()} {u.0.length()}\")\n",
        ),
        "3 4 6",
    );
}

#[test]
fn test_a_write_through_a_nested_tuple_reaches_only_the_inner_list() {
    assert_runs_with_output(
        &list_program(
            "    var a = List([1, 2, 3])\n    var t = ((a, 1), 2)\n    var u = t\n    t.0.0.push(9)\n    println(f\"{a.length()} {t.0.0.length()} {u.0.0.length()}\")\n",
        ),
        "3 4 3",
    );
}

#[test]
fn test_an_indexed_store_into_a_list_in_a_tuple_leaves_the_list_it_was_built_from() {
    assert_runs_with_output(
        &list_program(
            "    var a = List([1, 2, 3])\n    var t = (a, 1)\n    t.0[0] = 100\n    t.0.set(1, 200)\n    println(f\"{a[0]} {a[1]} {t.0[0]} {t.0[1]}\")\n",
        ),
        "1 2 100 200",
    );
}

#[test]
fn test_a_write_into_a_tuple_held_by_a_map_leaves_every_other_holder_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list
use system.collections.map

fn main()
    var a = List([1, 2, 3])
    var m = Map<String, ([int], int)>()
    m.set("k", (a, 1))
    var keep = m["k"]
    m["k"].0.push(9)
    println(f"{a.length()} {keep.0.length()} {m['k'].0.length()}")
"#,
        "3 3 4",
    );
}

#[test]
fn test_a_write_into_a_tuple_nothing_else_holds_lands_in_place() {
    assert_runs_with_output(
        &list_program(
            "    var t = (List([1, 2, 3]), 1)\n    t.0.resize(6, 9)\n    t.0.push(4)\n    println(f\"{t.0.length()} {t.1}\")\n",
        ),
        "7 1",
    );
}

#[test]
fn test_a_write_into_a_tuple_nothing_else_holds_copies_nothing() {
    let line = super::rc_stats::rc_stats(
        &list_program(
            "    var t = (List([1, 2, 3]), 1)\n    t.0.push(4)\n    println(f\"{t.0.length()}\")\n",
        ),
        &[],
    );
    assert!(line.contains("cow_copy=0"), "{line}");
}

#[test]
fn test_storing_into_a_list_in_a_tuple_returned_by_a_call_is_refused() {
    assert_compiler_error(
        "use system.collections.list\n\nfn mk(a [int]) ([int], int): (a, 1)\n\nfn main()\n    var a = List([1, 2, 3])\n    mk(a).0[0] = 5\n",
        "Immutability Violation",
    );
}
