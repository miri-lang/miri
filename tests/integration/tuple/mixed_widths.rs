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
