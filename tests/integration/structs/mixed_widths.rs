// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A struct whose fields have different widths is built where every read,
//! write, copy and element access of it looks for each field.

use super::utils::*;

const MIXED: &str = r#"
use system.io
use system.collections.list

struct Mixed
    a i8
    b int
    c f32

fn bumped(m Mixed) Mixed
    return Mixed(a: m.a + 1, b: m.b * 2, c: m.c + 0.5)

fn main()
    var m = Mixed(a: 7, b: 1234567890123, c: 2.5)
    println(f"{m.a} {m.b} {m.c}")
    m.a = -3
    m.b = -42
    m.c = 9.25
    println(f"{m.a} {m.b} {m.c}")
    var copy = m
    copy.a = 100
    println(f"{m.a} {copy.a} {copy.b} {copy.c}")
    let xs = List<Mixed>()
    xs.push(m)
    xs.push(Mixed(a: -128, b: 9223372036854775807, c: -0.125))
    println(f"{xs[0].a} {xs[0].b} {xs[0].c} {xs[1].a} {xs[1].b} {xs[1].c}")
    let r = bumped(m)
    println(f"{r.a} {r.b} {r.c}")
"#;

#[test]
fn a_mixed_width_struct_round_trips_through_every_use() {
    assert_runs_with_output(
        MIXED,
        "7 1234567890123 2.5\n-3 -42 9.25\n-3 100 -42 9.25\n\
         -3 -42 9.25 -128 9223372036854775807 -0.125\n-2 -84 9.75",
    );
}

#[test]
fn a_mixed_width_struct_round_trips_under_the_heap_guard() {
    assert_heap_guard_output(
        MIXED,
        "7 1234567890123 2.5\n-3 -42 9.25\n-3 100 -42 9.25\n\
         -3 -42 9.25 -128 9223372036854775807 -0.125\n-2 -84 9.75",
    );
}

#[test]
fn wide_and_bool_fields_keep_their_values_beside_narrow_ones() {
    assert_runs_with_output(
        r#"
use system.io

struct Wide
    on bool
    big i128
    off bool
    small u8

fn main()
    let top i128 = 170141183460469231731687303715884105727
    var w = Wide(on: true, big: top, off: false, small: 255)
    println(f"{w.on} {w.off} {w.small} {w.big == top}")
    w.off = true
    w.small = 1
    println(f"{w.on} {w.off} {w.small} {w.big == top}")
"#,
        "true false 255 true\ntrue true 1 true",
    );
}

#[test]
fn a_value_too_wide_for_a_narrow_field_is_refused() {
    assert_compiler_error(
        r#"
use system.io

struct Mixed
    a i8
    b int
    c f32

fn main()
    let m = Mixed(a: 300, b: 1, c: 2.5)
    println(f"{m.a}")
"#,
        "out of range",
    );
}
