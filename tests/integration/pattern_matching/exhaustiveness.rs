// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A match on a value that is neither an enum nor an option must give every
//! value an arm, in statement position as in expression position.

use super::utils::*;

const NON_EXHAUSTIVE: &str = "Non-exhaustive match on";

#[test]
fn test_guarded_int_arm_alone_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    let x = 1
    let r = match x
        n if n > 3: 1
    print(f"{r}")
"#,
        NON_EXHAUSTIVE,
    );
}

#[test]
fn test_int_literal_arms_without_catch_all_are_refused() {
    assert_compiler_error(
        r#"
fn main()
    let x = 1
    let r = match x
        3: 1
        4: 2
    print(f"{r}")
"#,
        NON_EXHAUSTIVE,
    );
}

#[test]
fn test_statement_int_match_without_catch_all_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    let x = 1
    match x
        n if n > 3: print("big")
    print("end")
"#,
        NON_EXHAUSTIVE,
    );
}

#[test]
fn test_string_match_without_catch_all_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    let s = "a"
    let r = match s
        "b": 1
    print(f"{r}")
"#,
        NON_EXHAUSTIVE,
    );
}

#[test]
fn test_function_returning_incomplete_match_is_refused() {
    assert_compiler_error(
        r#"
fn name(x int) String
    return match x
        1: "one"
        2: "two"

fn main()
    print(name(7))
"#,
        NON_EXHAUSTIVE,
    );
}

#[test]
fn test_bool_match_missing_false_is_refused_and_names_it() {
    assert_compiler_error(
        r#"
fn main()
    let b = false
    let q = match b
        true: 10
    print(f"{q}")
"#,
        "Non-exhaustive match on bool. Missing values: false",
    );
}

#[test]
fn test_bool_arm_with_guard_does_not_count() {
    assert_compiler_error(
        r#"
fn main()
    let b = false
    let c = true
    let q = match b
        true if c: 10
        false: 20
    print(f"{q}")
"#,
        "Non-exhaustive match on bool. Missing values: true",
    );
}

#[test]
fn test_tuple_match_without_full_cover_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    let t = (1, 2)
    let w = match t
        (1, 3): "a"
    print(f"[{w}]")
"#,
        NON_EXHAUSTIVE,
    );
}

#[test]
fn test_tuple_of_bools_missing_a_combination_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    let t = (true, false)
    let w = match t
        (true, true): 1
        (true, false): 2
        (false, true): 3
    print(f"{w}")
"#,
        NON_EXHAUSTIVE,
    );
}

#[test]
fn test_float_match_without_catch_all_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    let f = 1.5
    let w = match f
        1.5: 1
    print(f"{w}")
"#,
        NON_EXHAUSTIVE,
    );
}

#[test]
fn test_bool_match_with_both_arms_runs() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let b = false
    let q = match b
        true: 10
        false: 20
    println(f"{q}")
"#,
        "20",
    );
}

#[test]
fn test_bool_alternatives_cover_both_values() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let b = false
    let q = match b
        true | false: 30
    println(f"{q}")
"#,
        "30",
    );
}

#[test]
fn test_int_match_with_underscore_catch_all_runs() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let x = 7
    let r = match x
        3: 1
        _: 2
    println(f"{r}")
"#,
        "2",
    );
}

#[test]
fn test_int_match_with_unguarded_binding_runs() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let x = 7
    let r = match x
        3: 1
        n: n + 1
    println(f"{r}")
"#,
        "8",
    );
}

#[test]
fn test_guarded_arms_with_catch_all_run() {
    assert_runs_with_output(
        r#"
use system.io

fn classify(x int) String
    return match x
        n if n > 3: "big"
        n if n < 0: "negative"
        _: "small"

fn main()
    println(classify(5))
    println(classify(-1))
    println(classify(2))
"#,
        "big\nnegative\nsmall",
    );
}

#[test]
fn test_string_match_with_catch_all_runs() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let s = "a"
    let r = match s
        "b": 1
        _: 2
    println(f"{r}")
"#,
        "2",
    );
}

#[test]
fn test_tuple_of_bools_covered_arm_by_arm_runs() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let t = (false, true)
    let w = match t
        (true, true): 1
        (true, false): 2
        (false, true): 3
        (false, false): 4
    println(f"{w}")
"#,
        "3",
    );
}

#[test]
fn test_tuple_of_enum_and_bool_covered_arm_by_arm_runs() {
    assert_runs_with_output(
        r#"
use system.io

enum Light
    Red
    Green

fn main()
    let t = (Light.Green, false)
    let w = match t
        (Light.Red, _): 1
        (Light.Green, true): 2
        (Light.Green, false): 3
    println(f"{w}")
"#,
        "3",
    );
}

#[test]
fn test_tuple_destructuring_binds_every_position_runs() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let t = (10, 20)
    let sum = match t
        (a, b): a + b
    println(f"{sum}")
"#,
        "30",
    );
}

#[test]
fn test_tuple_with_literal_arm_and_catch_all_runs() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let t = (1, 2)
    let w = match t
        (1, 3): "a"
        _: "other"
    println(w)
"#,
        "other",
    );
}
