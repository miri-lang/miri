// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A field read off a tuple element (`p.0.value`) is laid out by the element's
//! own type, not the tuple's: the struct, class or tuple the element holds
//! decides the field's offset.

use super::utils::*;

#[test]
fn test_a_struct_field_read_off_a_tuple_element_reads_that_field() {
    assert_heap_guard_output(
        r#"
use system.io

struct S
    value String
    n int

fn main()
    let r = (S(value: "e" + "f", n: 7), 3)
    println(r.0.value)
    println(f"{r.0.n} {r.1}")
"#,
        "ef\n7 3",
    );
}

#[test]
fn test_a_class_field_read_off_a_tuple_element_reads_that_field() {
    assert_heap_guard_output(
        r#"
use system.io

class Pair
    public var left int
    public var right String

class Box<T>
    public var value T

fn main()
    let q = (1, Pair(left: 5, right: "r" + "s"))
    println(f"{q.1.left} {q.1.right}")
    let p = (Box<String>(value: "a" + "b"), 1)
    println(p.0.value)
    let first = p.0
    println(first.value)
"#,
        "5 rs\nab\nab",
    );
}

#[test]
fn test_a_tuple_element_read_off_a_nested_tuple_reads_that_element() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let t = (1.5, (true, 42, "g" + "h"))
    println(f"{t.1.1} {t.1.2} {t.1.0}")
"#,
        "42 gh true",
    );
}
