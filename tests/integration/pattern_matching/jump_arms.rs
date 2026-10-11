// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A match arm whose body, written after the colon, leaves: `return`, `break`
//! or `continue` on the arm's own line.

use super::utils::*;

// TODO: the trailing `return -1` is unreachable. It is there because a
// function whose last statement is a match returning from every arm is refused
// as returning void; drop it once that is fixed, so this covers the tail form.
#[test]
fn test_return_after_arm_colon_returns_from_the_function() {
    assert_runs_with_output(
        r#"
fn first_or(x int?, fallback int) int
    match x
        Some(v): return v
        None: return fallback
    return -1

fn main()
    println(f"{first_or(Some(4), 9)} {first_or(None, 9)}")
"#,
        "4 9",
    );
}

#[test]
fn test_bare_return_after_arm_colon_leaves_a_void_function() {
    assert_runs_with_output(
        r#"
fn report(x int?)
    match x
        None: return
        Some(v): println(f"got {v}")
    println("done")

fn main()
    report(None)
    report(Some(2))
"#,
        "got 2\ndone",
    );
}

#[test]
fn test_continue_and_break_after_arm_colon_steer_the_loop() {
    assert_runs_with_output(
        r#"
fn main()
    var seen = 0
    for x in [1, 2, 3, 4, 5]
        match x
            2: continue
            4: break
            _: seen = seen + x
    println(f"{seen}")
"#,
        "4",
    );
}

#[test]
fn test_return_arm_in_a_match_that_initialises_a_binding() {
    assert_runs_with_output(
        r#"
fn parse_or_zero(text String) int
    let n = match text.to_int()
        Some(v): v
        None: return 0
    return n * 2

fn main()
    println(f"{parse_or_zero('21')} {parse_or_zero('x')}")
"#,
        "42 0",
    );
}

#[test]
fn test_return_after_arm_colon_still_type_checks_its_value() {
    assert_compiler_error(
        r#"
fn pick(x int?) int
    match x
        Some(v): return "text"
        None: return 0

fn main()
    println(f"{pick(None)}")
"#,
        "expected int, got String",
    );
}
