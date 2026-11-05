// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_match_variable() {
    assert_runs_with_output(
        r#"

let x = 42
let result = match x
    0: 0
    42: 1
    _: 2
print(f"{result}")
"#,
        "1",
    );
}

#[test]
fn test_match_identifier_binding() {
    assert_runs_with_output(
        r#"

let x = 42
let result = match x
    value: value * 2
print(f"{result}")
"#,
        "84",
    );
}

#[test]
fn test_match_with_guards() {
    assert_runs_with_output(
        r#"

let x = 15
let result = match x
    n if n > 10: 1
    n if n > 5: 2
    _: 3
print(f"{result}")
"#,
        "1",
    );
}

#[test]
fn test_match_guard_binding_used() {
    assert_runs_with_output(
        r#"

let x = 15
let result = match x
    n if n >= 10: n * 2
    n if n >= 5: n + 10
    n: n
print(f"{result}")
"#,
        "30",
    );
}

#[test]
fn test_match_guard_second_arm() {
    assert_runs_with_output(
        r#"

let x = 7
let result = match x
    n if n >= 10: n * 2
    n if n >= 5: n + 10
    n: n
print(f"{result}")
"#,
        "17",
    );
}

#[test]
fn test_match_guard_catch_all() {
    assert_runs_with_output(
        r#"

let x = 3
let result = match x
    n if n >= 10: n * 2
    n if n >= 5: n + 10
    n: n
print(f"{result}")
"#,
        "3",
    );
}

/// A guard that fails leaves an arm whose names are already bound; the names
/// it bound are released on the way to the next arm.
#[test]
fn test_a_failing_guard_releases_the_names_its_arm_bound() {
    assert_heap_guard_output(
        r#"
enum Pair
    Left(int, String)
    Right(String, int)

fn show(p Pair) String
    match p
        Pair.Left(n, s) if n > 3: f"big {n} {s}"
        Pair.Left(n, s): f"small {n} {s}"
        Pair.Right(s, n): f"right {s} {n}"

fn main()
    let a = show(Pair.Left(1, "x" + ""))
    let b = show(Pair.Left(5, "y" + ""))
    println(f"{a}|{b}")
"#,
        "small 1 x|big 5 y",
    );
}

#[test]
fn test_none_skips_a_guarded_some_arm_for_the_wildcard() {
    // No arm names `None`, so it belongs to the `_` arm. The `Some` arm before
    // it binds the payload, which `None` does not have.
    assert_runs_with_output(
        r#"
fn pick(x int?) int
    match x
        Some(c) if c == 1: c
        _: 7

fn main()
    println(f"{pick(None)} {pick(1)} {pick(2)}")
"#,
        "7 1 7",
    );
}

/// A guard is an ordinary condition: an enum compared with `==` there agrees
/// with the same comparison outside the `match`.
#[test]
fn test_enum_equality_in_a_guard_agrees_with_the_same_comparison_outside() {
    assert_runs_with_output(
        r#"
use system.collections.list

enum Instr
    Set(int, int)
    Halt

fn main()
    let code = List([Instr.Set(1, 2), Instr.Set(2, 4), Instr.Halt])
    let x = 2
    let outside = code[x] == Instr.Halt
    match x
        a if code[a] == Instr.Halt: println(f"{outside} halt")
        _: println(f"{outside} other")
    match 0
        a if code[a] != Instr.Halt: println("not halt")
        _: println("halt")
"#,
        "true halt\nnot halt",
    );
}

#[test]
fn test_a_free_function_call_in_a_guard() {
    assert_runs_with_output(
        r#"
fn is_two(n int) bool
    return n == 2

fn main()
    for x in 1..4
        match x
            a if is_two(a): println(f"{a} two")
            a: println(f"{a} other")
"#,
        "1 other\n2 two\n3 other",
    );
}

#[test]
fn test_a_method_call_in_a_guard() {
    assert_runs_with_output(
        r#"
fn main()
    let s = "ab"
    for x in 1..4
        match x
            a if s.length() == a: println(f"{a} len")
            a: println(f"{a} no")
"#,
        "1 no\n2 len\n3 no",
    );
}
