// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A narrower number handed to `Some` where a wider optional is declared is
//! widened on the way, as a plain assignment widens it: its value, sign
//! included, survives.

use super::super::utils::*;

const SHOW: &str = r#"
fn show(o i64?)
    match o
        Some(v): println(f"{v}")
        None: println("none")
"#;

#[test]
fn test_some_of_a_narrow_integer_passed_where_a_wider_optional_is_declared_keeps_its_value() {
    assert_runs_with_output(
        &format!(
            "{SHOW}{}",
            r#"
fn main()
    let b i8 = -5
    show(Some(b))
"#
        ),
        "-5",
    );
}

#[test]
fn test_some_of_a_narrow_integer_stored_at_a_wider_optional_keeps_its_value() {
    assert_runs_with_output(
        &format!(
            "{SHOW}{}",
            r#"
fn main()
    let c i32 = -70000
    let p i64? = Some(c)
    show(p)
    let u u8 = 200
    let q i64? = Some(u)
    show(q)
"#
        ),
        "-70000\n200",
    );
}

#[test]
fn test_some_of_a_narrow_float_stored_at_a_wider_optional_keeps_its_value() {
    assert_runs_with_output(
        r#"
fn main()
    let f f32 = 1.5
    let o f64? = Some(f)
    match o
        Some(v): println(f"{v}")
        None: println("none")
"#,
        "1.5",
    );
}

#[test]
fn test_some_of_a_narrow_integer_in_a_literal_of_wider_optionals_keeps_its_value() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn main()
    let b i8 = -5
    let xs Array<i64?, 1> = [Some(b)]
    match xs[0]
        Some(v): println(f"{v}")
        None: println("none")
"#,
        "-5",
    );
}
