// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_match_or_patterns_first() {
    assert_runs_with_output(
        r#"

let x = 1
let result = match x
    1 | 2 | 3: 10
    4 | 5: 20
    _: 99
print(f"{result}")
"#,
        "10",
    );
}

#[test]
fn test_match_or_patterns_second_arm() {
    assert_runs_with_output(
        r#"

let x = 5
let result = match x
    1 | 2 | 3: 10
    4 | 5: 20
    _: 99
print(f"{result}")
"#,
        "20",
    );
}

#[test]
fn test_match_or_patterns_default() {
    assert_runs_with_output(
        r#"

let x = 7
let result = match x
    1 | 2 | 3: 10
    4 | 5: 20
    _: 99
print(f"{result}")
"#,
        "99",
    );
}

// Regression: String alt-patterns, first alternative matches
#[test]
fn test_match_string_alt_patterns_first() {
    assert_runs_with_output(
        r#"
let subject = "abc"
let result = match subject
    "xyz" | "abc" | "def": "hit_alt"
    _: "other"
println(result)
"#,
        "hit_alt",
    );
}

// Regression: String alt-patterns, second alternative matches
#[test]
fn test_match_string_alt_patterns_second() {
    assert_runs_with_output(
        r#"
let subject = "def"
let result = match subject
    "abc" | "def": "hit_alt"
    _: "other"
println(result)
"#,
        "hit_alt",
    );
}

// Regression: Regex alt-patterns, first alternative matches
#[test]
fn test_match_regex_alt_patterns_first() {
    assert_runs_with_output(
        r#"
use system.text

let subject = "999"
let result = match subject
    re"^[a-z]+$" | re"^\d+$": "hit_alt"
    _: "other"
println(result)
"#,
        "hit_alt",
    );
}

// Regression: Regex alt-patterns, second alternative matches
#[test]
fn test_match_regex_alt_patterns_second() {
    assert_runs_with_output(
        r#"
use system.text

let subject = "abc"
let result = match subject
    re"^\d+$" | re"^[a-z]+$": "hit_alt"
    _: "other"
println(result)
"#,
        "hit_alt",
    );
}

#[test]
fn test_a_name_bound_in_only_one_alternative_is_refused() {
    assert_compiler_error(
        r#"
fn show(o String?) String
    match o
        Some(s) | None: f"[{s}]"

fn main()
    println(show(None))
"#,
        "'s' is bound in some alternatives of this arm but not in all of them",
    );
}

#[test]
fn test_a_name_bound_at_two_types_across_alternatives_is_refused() {
    assert_compiler_error(
        r#"
enum E
    A(int)
    B(String)

fn show(e E) String
    match e
        E.A(x) | E.B(x): "k"

fn main()
    println(show(E.B("z")))
"#,
        "'x' is bound at int in one alternative of this arm and at String in another",
    );
}

#[test]
fn test_alternatives_that_bind_nothing_may_discard_payloads() {
    assert_runs_with_output(
        r#"
fn show(o int?) String
    match o
        Some(_) | None: "any"

fn main()
    println(f"{show(Some(1))}|{show(None)}")
"#,
        "any|any",
    );
}

#[test]
fn test_alternatives_with_a_guard_bind_from_the_alternative_that_matched() {
    assert_heap_guard_output(
        r#"
enum Pair
    Left(int, String)
    Right(String, int)

fn show(p Pair) String
    match p
        Pair.Left(n, s) | Pair.Right(s, n) if n > 3: f"big {n} {s}"
        Pair.Left(n, s) | Pair.Right(s, n): f"small {n} {s}"

fn main()
    var out = ""
    var i = 0
    while i < 3
        let left = show(Pair.Left(i * 3, "x" + ""))
        let right = show(Pair.Right("y" + "", i * 3))
        out = out + f"{left},{right};"
        i = i + 1
    println(out)
"#,
        "small 0 x,small 0 y;small 3 x,small 3 y;big 6 x,big 6 y;",
    );
}

#[test]
fn test_tuple_alternatives_test_their_string_elements() {
    assert_heap_guard_output(
        r#"
fn show(t (String, int)) int
    match t
        ("a", n) | ("b", n): n
        (_, n): 0 - n

fn main()
    let a = "a" + ""
    let b = "b" + ""
    let c = "c" + ""
    println(f"{show((a, 1))}|{show((b, 2))}|{show((c, 3))}")
"#,
        "1|2|-3",
    );
}

#[test]
fn test_an_option_none_alternative_is_tested_beside_a_nested_literal() {
    assert_runs_with_output(
        r#"
fn show(o int?) String
    match o
        Some(0) | Option.None: "zero or none"
        Some(n): f"n {n}"

fn main()
    println(f"{show(Some(0))}|{show(None)}|{show(Some(4))}")
"#,
        "zero or none|zero or none|n 4",
    );
}
