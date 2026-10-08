// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A struct's fields are known before any body is checked, so a body written
//! above the struct — a free function or a class method — can read its
//! fields and build it by field name.

use super::utils::*;

#[test]
fn an_earlier_function_reads_a_field_of_a_later_struct_parameter() {
    assert_runs_with_output(
        r#"
use system.io

fn gets(b Box) int
    return b.w

fn main()
    println(f"{gets(Box(w: 2))}")

struct Box
    w int
"#,
        "2\n",
    );
}

#[test]
fn an_earlier_function_builds_a_later_struct_by_field_name() {
    assert_runs_with_output(
        r#"
use system.io

fn make(n int) Box
    return Box(w: n + 1, h: n * 2)

fn main()
    let b = make(3)
    println(f"{b.w} {b.h}")

struct Box
    w int
    h int
"#,
        "4 6\n",
    );
}

#[test]
fn a_field_of_a_later_struct_returned_from_a_call_is_read() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    println(f"{make().label}")

fn make() Tag
    return Tag(label: "late")

struct Tag
    label String
"#,
        "late\n",
    );
}

#[test]
fn an_earlier_class_method_reads_a_field_of_a_later_struct() {
    assert_runs_with_output(
        r#"
use system.io

class Q
    var n int
    fn area(b Box) int
        return b.w * b.h + self.n

struct Box
    w int
    h int

fn main()
    let q = Q(n: 1)
    println(f"{q.area(Box(w: 2, h: 3))}")
"#,
        "7\n",
    );
}

#[test]
fn a_later_struct_holding_another_later_struct_is_read_through() {
    assert_runs_with_output(
        r#"
use system.io

fn corner(r Rect) int
    return r.origin.x + r.origin.y

fn main()
    println(f"{corner(Rect(origin: Point(x: 4, y: 5)))}")

struct Rect
    origin Point

struct Point
    x int
    y int
"#,
        "9\n",
    );
}

#[test]
fn a_struct_declared_twice_is_reported_as_already_defined() {
    assert_compiler_error(
        r#"
struct Box
    w int

struct Box
    h int

fn main()
    let b = Box(w: 1)
"#,
        "Type 'Box' is already defined",
    );
}

#[test]
fn a_missing_field_of_a_later_struct_is_still_refused() {
    assert_compiler_error(
        r#"
fn gets(b Box) int
    return b.h

fn main()
    let x = gets(Box(w: 2))

struct Box
    w int
"#,
        "has no field 'h'",
    );
}

#[test]
fn a_class_field_initializer_builds_a_later_struct() {
    assert_runs_with_output(
        r#"
use system.io

class Holder
    var b = Box(w: 5)

fn main()
    let h = Holder()
    println(f"{h.b.w}")

struct Box
    w int
"#,
        "5\n",
    );
}
