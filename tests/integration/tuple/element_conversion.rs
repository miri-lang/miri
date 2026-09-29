// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A tuple's elements are compared invariantly: a tuple value is stored at its
//! own layout and nothing converts it, so `(int, String)` is not an
//! `(int?, String)`. A tuple literal still builds at a declared type whose
//! elements it fits, each element stored at its declared type.

use super::utils::*;

#[test]
fn test_a_tuple_value_is_not_read_at_a_wider_element_type() {
    assert_compiler_error(
        r#"
use system.io

fn main()
    let t = (7, "a" + "b")
    let u (int?, String) = t
    println(u.1)
"#,
        "Type mismatch for variable 'u'",
    );
}

#[test]
fn test_a_tuple_of_narrow_integers_is_not_read_at_wider_ones() {
    assert_compiler_error(
        r#"
fn main()
    let b i8 = 5
    let t = (b, 1)
    let u (i64, int) = t
"#,
        "Type mismatch for variable 'u'",
    );
}

#[test]
fn test_a_tuple_literal_builds_at_a_declared_optional_element_type() {
    assert_heap_guard_output(
        r#"
use system.io

fn pair() (int?, String)
    return (7, "r" + "s")

fn show(u (int?, String))
    println(f"{u.0 ?? -1} {u.1}")

fn main()
    let s = "a"
    let u (int?, String) = (7, f"{s}b")
    println(f"{u.0 ?? -1} {u.1}")
    let n int? = None
    let v (int?, String) = (n, "x")
    println(f"{v.0 ?? -1} {v.1}")
    let p = pair()
    println(f"{p.0 ?? -1} {p.1}")
    show((9, "arg"))
"#,
        "7 ab\n-1 x\n7 rs\n9 arg",
    );
}

#[test]
fn test_a_declared_tuple_with_an_optional_element_releases_its_box() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let u (int?, String) = (Some(7), "x")
    let w (String?, int) = (Some("y" + "z"), 1)
    let text = w.0 ?? "none"
    println(f"{u.0 ?? -1} {text}")
"#,
        "7 yz",
    );
}
