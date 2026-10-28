// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Optionals and small values held as their bytes, cleared, set and copied
//! member by member. Each program pins the values it prints, so a member
//! the clear or the copy skips, or reads at the wrong offset, shows as a
//! wrong number rather than only as a crash.

use super::utils::*;

#[test]
fn an_optional_int_round_trips_through_none_and_some_copies() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var o int? = None
    let a = o
    o = 7
    let b = o
    o = None
    let c = o
    println(f"{a} {b} {c}")
"#,
        "None Some(7) None",
    );
}

#[test]
fn an_optional_float_round_trips_through_none_and_some_copies() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var o float? = 2.5
    let a = o
    o = None
    let b = o
    o = -0.25
    println(f"{a} {b} {o}")
"#,
        "Some(2.5) None Some(-0.25)",
    );
}

#[test]
fn an_optional_copied_out_of_a_struct_field_keeps_its_tag_and_payload() {
    assert_runs_with_output(
        r#"
use system.io

struct Slot
    id int
    value int?

fn main()
    var s = Slot(id: 3, value: None)
    let before = s.value
    s.value = 42
    let after = s.value
    let copy = s
    s.value = None
    println(f"{before} {after} {copy.id} {copy.value} {s.value}")
"#,
        "None Some(42) 3 Some(42) None",
    );
}

#[test]
fn a_struct_of_an_int_and_a_bool_copied_keeps_both_members() {
    assert_runs_with_output(
        r#"
use system.io

struct Tag
    id int
    live bool

fn main()
    var t = Tag(id: 5, live: true)
    let c = t
    t = Tag(id: -1, live: false)
    println(f"{c.id} {c.live} {t.id} {t.live}")
"#,
        "5 true -1 false",
    );
}

#[test]
fn a_tuple_of_an_int_and_a_float_copied_keeps_both_elements() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var t = (4, 0.75)
    let c = t
    t = (9, 1.5)
    println(f"{c.0} {c.1} {t.0} {t.1}")
"#,
        "4 0.75 9 1.5",
    );
}

#[test]
fn a_nested_optional_copied_keeps_each_layer() {
    assert_runs_with_output(
        r#"
use system.io

fn show(o Option<int?>)
    match o
        Some(inner): match inner
            Some(v): println(f"some some {v}")
            None: println("some none")
        None: println("none")

fn main()
    let inner int? = None
    var o Option<int?> = inner
    let a = o
    o = Some(Some(6))
    let b = o
    o = None
    show(a)
    show(b)
    show(o)
"#,
        "some none\nsome some 6\nnone",
    );
}

#[test]
fn an_optional_struct_of_mixed_widths_cleared_and_copied_keeps_every_member() {
    assert_runs_with_output(
        r#"
use system.io

struct Wide
    a i8
    b i128
    c f32

fn main()
    var w Wide? = None
    let before = w
    w = Wide(a: 3, b: 170141183460469231731687303715884105727, c: 0.5)
    let after = w
    if let Some(x) = before
        println("present before")
    if let Some(x) = after
        println(f"{x.a} {x.b == 170141183460469231731687303715884105727} {x.c}")
"#,
        "3 true 0.5",
    );
}
