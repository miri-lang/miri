// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `x in r` on a range of numbers holds exactly when `for v in r` visits `x`:
//! at or above the start, and below the end, or at it when the end is
//! included.

use super::utils::*;

#[test]
fn an_exclusive_range_holds_its_start_but_not_its_end() {
    assert_runs_with_output(
        r#"
fn main()
    var out = ""
    for i in -1..12
        if i in 0..10
            out = out + "e"
        else
            out = out + "."
    println(out)
"#,
        ".eeeeeeeeee..",
    );
}

#[test]
fn an_inclusive_range_holds_its_end() {
    assert_runs_with_output(
        r#"
fn main()
    var out = ""
    for i in -1..12
        if i in 0..=10
            out = out + "i"
        else
            out = out + "."
    println(out)
"#,
        ".iiiiiiiiiii.",
    );
}

#[test]
fn a_range_value_answers_by_its_own_bounds() {
    assert_runs_with_output(
        r#"
fn main()
    let r = 3..5
    let a i8 = 4
    println(f"{a in r} {5 in r} {2 in r} {4 in 5..3}")
"#,
        "true false false false",
    );
}

#[test]
fn a_generic_body_tests_membership_at_each_instantiation() {
    assert_runs_with_output(
        r#"
fn within<T>(x T, r Range<T>) bool
    return x in r

fn main()
    println(f"{within(4, 3..5)} {within(1.5, 1.0..2.0)} {within(2.0, 1.0..2.0)}")
"#,
        "true true false",
    );
}

#[test]
fn the_value_is_read_before_the_range_is_built() {
    assert_runs_with_output(
        r#"
fn grow(n out int) int
    n = 99
    return 10

fn main()
    var n = 5
    println(f"{n in 0..grow(n)} {n}")
"#,
        "true 99",
    );
}

#[test]
fn a_parameter_guard_on_a_range_admits_values_inside_it() {
    assert_runs_with_output(
        r#"
fn pick(x int in 0..10) int
    return x * 2

fn main()
    println(f"{pick(0)} {pick(9)}")
"#,
        "0 18",
    );
}

#[test]
fn a_parameter_guard_on_a_range_refuses_its_excluded_end() {
    assert_runtime_error(
        r#"
fn pick(x int in 0..10) int
    return x * 2

fn main()
    println(f"{pick(10)}")
"#,
        "pick: x must be in 0..10",
    );
}
