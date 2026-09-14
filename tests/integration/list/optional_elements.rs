// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Storing and reading back optional elements.
//!
//! A bare value stored into a `T?` slot has to arrive as `Some(value)`. Every
//! store below hands the collection a bare payload, so each test fails the same
//! way if the store keeps the raw value: the read then treats the payload as a
//! pointer to an optional and faults. The tests pin the printed value rather
//! than only that the program ran.

use super::utils::*;

#[test]
fn pushed_int_reads_back_as_some() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var xs = List<int?>()
    xs.push(1)
    println(f"{xs.length()}")
    let v = xs[0]
    println(f"{v}")
"#,
        "1\nSome(1)",
    );
}

#[test]
fn pushed_string_reads_back_as_some() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var xs = List<String?>()
    let s = "kept"
    xs.push(s)
    xs.push("made " + "here")
    println(f"{xs[0]} {xs[1]} {s}")
"#,
        "Some(kept) Some(made here) kept",
    );
}

#[test]
fn element_at_reads_an_optional_element() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var xs = List<int?>()
    xs.push(7)
    var ys = List<String?>()
    ys.push("seven")
    println(f"{xs.element_at(0)} {ys.element_at(0)}")
"#,
        "Some(7) Some(seven)",
    );
}

#[test]
fn for_loop_visits_none_and_payload_elements() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var xs = List<int?>()
    xs.push(1)
    xs.push(None)
    xs.push(3)
    for x in xs
        match x
            Some(n): println(f"n={n}")
            None: println("none")
    var ys = List<String?>()
    ys.push(None)
    ys.push("b")
    for y in ys
        println(f"{y}")
"#,
        "n=1\nnone\nn=3\nNone\nSome(b)",
    );
}

#[test]
fn empty_optional_list_has_no_first_element() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let xs = List<int?>()
    println(f"{xs.length()}")
    match xs.first()
        Some(_inner): println("first present")
        None: println("first missing")
    for x in xs
        println(f"{x}")
    println("done")
"#,
        "0\nfirst missing\ndone",
    );
}

#[test]
fn insert_set_and_index_assign_wrap_the_stored_value() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var xs = List<int?>()
    xs.push(None)
    xs.push(None)
    xs.insert(0, 5)
    xs.set(1, 6)
    xs[2] = 7
    println(f"{xs[0]} {xs[1]} {xs[2]}")
    var ys = List<String?>()
    ys.push(None)
    ys.insert(0, "a")
    ys.set(1, "b")
    ys[0] = "c"
    println(f"{ys[0]} {ys[1]}")
"#,
        "Some(5) Some(6) Some(7)\nSome(c) Some(b)",
    );
}

#[test]
fn optional_list_stays_readable_after_a_read_under_heap_guard() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    var xs = List<String?>()
    xs.push("one")
    xs.push(None)
    let s = "two"
    xs.push(s)
    let first = xs[0]
    println(f"{first}")
    for x in xs
        println(f"{x}")
    println(f"{xs[0]} {xs[2]} {s}")
"#,
        "Some(one)\nSome(one)\nNone\nSome(two)\nSome(one) Some(two) two",
    );
}

#[test]
fn pushing_a_mismatched_payload_is_rejected() {
    assert_compiler_error(
        r#"
use system.collections.list

fn main()
    var xs = List<int?>()
    xs.push("one")
"#,
        "expected int?, got String",
    );
}
