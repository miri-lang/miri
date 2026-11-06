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

#[test]
fn a_written_range_evaluates_the_value_and_each_bound_once_left_to_right() {
    assert_runs_with_output(
        r#"
fn note(log out String, tag String, v int) int
    log = log + tag
    return v

fn main()
    var log = ""
    let a = note(log, "x", 3) in note(log, "a", 3)..note(log, "b", 5)
    let b = note(log, "x", 5) in note(log, "a", 3)..=note(log, "b", 5)
    let c = note(log, "x", 5) in note(log, "a", 3)..note(log, "b", 5)
    println(f"{a} {b} {c} {log}")
"#,
        "true true false xabxabxab",
    );
}

#[test]
fn a_written_range_holds_an_i8_a_float_and_a_negative_value_by_its_bounds() {
    assert_runs_with_output(
        r#"
fn main()
    let w i8 = 4
    let lo = 2
    let hi = 4
    println(f"{w in lo..hi} {w in lo..=hi} {1.5 in 1.0..2.0} {-1 in -1..0}")
"#,
        "false true true true",
    );
}

#[test]
fn a_written_range_tested_in_a_cpu_forall_holds_by_its_bounds() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn main()
    var dst = Array<int, 8>()
    forall i in 0..8
        if i in 2..5: dst[i] = 1
        if i in 6..=7: dst[i] = 2
    var out = ""
    for i in 0..8
        out = out + f"{dst[i]}"
    println(out)
"#,
        "00111022",
    );
}

#[test]
fn a_float_is_in_a_range_of_f32_exactly_when_the_comparisons_say_so() {
    assert_runs_with_output(
        r#"
fn main()
    let lo f32 = -0.5
    let hi f32 = 0.5
    let inside float = 0.25
    let past float = 0.5000000001
    println(f"{inside in lo..=hi} {past in lo..=hi} {past >= lo and past <= hi}")
"#,
        "true false false",
    );
}

#[test]
fn a_range_value_whose_end_was_written_tests_against_the_new_end() {
    assert_runs_with_output(
        r#"
fn main()
    var r = 0..3
    let before = 4 in r
    r.end = 5
    println(f"{before} {4 in r} {5 in r}")
"#,
        "false true false",
    );
}

#[test]
fn membership_refuses_a_value_the_comparisons_refuse() {
    assert_compiler_error(
        r#"
fn main()
    let s = "b"
    println(f"{s in 1..3}")
"#,
        "cannot check membership of String in range of int",
    );
}

#[test]
fn a_float_local_is_tested_against_float_literal_bounds() {
    assert_runs_with_output(
        r#"
fn main()
    var out = ""
    for i in 0..8
        let x = (i as float) * 0.25 - 1.0
        if x in -0.5..=0.5
            out = out + "1"
        else
            out = out + "0"
    println(out)
"#,
        "00111110",
    );
}
