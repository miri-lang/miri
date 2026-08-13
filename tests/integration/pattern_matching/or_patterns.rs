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
