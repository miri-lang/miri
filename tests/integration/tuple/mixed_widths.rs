// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A tuple whose elements have different widths is built where every read,
//! copy, element access and call of it looks for each element.

use super::utils::*;

#[test]
fn a_mixed_width_tuple_round_trips_through_every_use() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn bumped(t (i8, int, f32)) (i8, int, f32)
    return (t.0 + 1, t.1 * 2, t.2 + 0.5)

fn main()
    var t = (7 as i8, 1234567890123, 2.5 as f32)
    println(f"{t.0} {t.1} {t.2}")
    t = (-3 as i8, -42, 9.25 as f32)
    println(f"{t.0} {t.1} {t.2}")
    let copy = t
    println(f"{copy.0} {copy.1} {copy.2}")
    let xs = List<(i8, int, f32)>()
    xs.push(t)
    xs.push((-100 as i8, 9223372036854775807, -0.125 as f32))
    println(f"{xs[0].0} {xs[0].1} {xs[0].2} {xs[1].0} {xs[1].1} {xs[1].2}")
    let r = bumped(t)
    println(f"{r.0} {r.1} {r.2}")
"#,
        "7 1234567890123 2.5\n-3 -42 9.25\n-3 -42 9.25\n\
         -3 -42 9.25 -100 9223372036854775807 -0.125\n-2 -84 9.75",
    );
}

#[test]
fn a_tuple_element_store_stays_refused() {
    assert_compiler_error(
        r#"
fn main()
    var t = (7 as i8, 1234567890123, 2.5 as f32)
    t.1 = 5
"#,
        "tuple",
    );
}

#[test]
fn a_tuple_literal_takes_the_widths_its_declared_type_names() {
    assert_runs_with_output(
        r#"
fn half(p (u8, f32)) f32
    return p.1 / 2.0

fn widest() (i128, u64)
    return (170141183460469231731687303715884105727, 18446744073709551615)

fn main()
    let a (i8, i8) = (3, -4)
    let b (i8?, i8) = (3, 4)
    let c (f32, int) = (1.5, 4)
    let d (u8, (i16, f32)) = (200, (-300, 0.25))
    let e = widest()
    println(f"{a.0 + a.1} {b.1} {c.0} {d.0} {d.1.0} {d.1.1} {e.1}")
    println(f"{e.0 == 170141183460469231731687303715884105727} {half((7, 2.5))}")
    match b.0
        Some(v): println(f"{v}")
        None: println("none")
"#,
        "-1 4 1.5 200 -300 0.25 18446744073709551615\ntrue 1.25\n3",
    );
}

#[test]
fn a_tuple_literal_element_out_of_its_declared_width_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    let t (i8, i8) = (3, 300)
    println(f"{t.0}")
"#,
        "Integer literal '300' is out of range for i8",
    );
}
