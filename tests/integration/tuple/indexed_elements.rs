// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An element of a tuple picked by a value — `t[i]`, `element_at`, a `for`
//! loop, `contains` — is retained by whoever takes it, so a counted element is
//! released once by each holder and never twice by one.

use super::utils::*;

#[test]
fn test_a_string_picked_by_a_value_is_retained() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let x = ("a" + "b", "c" + "d")
    var i = 1
    let y = x[i]
    println(y)
"#,
        "cd",
    );
}

#[test]
fn test_a_string_read_through_element_at_is_retained() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.tuple

fn main()
    let x = ("a" + "b", "c" + "d")
    let y = x.element_at(1)
    println(y)
"#,
        "cd",
    );
}

#[test]
fn test_a_for_loop_over_a_tuple_of_strings_retains_each() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.tuple

fn main()
    let x = ("a" + "b", "c" + "d")
    for s in x
        println(s)
"#,
        "ab\ncd",
    );
}

#[test]
fn test_a_class_instance_picked_by_a_value_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.io

class Res
    var id int
    fn drop(self)
        println(f"drop {self.id}")

fn main()
    let t = (Res(id: 1), Res(id: 2))
    var i = 1
    let r = t[i]
    println(f"got {r.id}")
"#,
        "got 2\ndrop 1\ndrop 2",
    );
}

#[test]
fn test_contains_on_a_tuple_of_strings_releases_nothing_twice() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.tuple

fn main()
    let x = ("a" + "b", "c" + "d")
    if x.contains("zz")
        println("never")
    println("ok")
"#,
        "ok",
    );
}
