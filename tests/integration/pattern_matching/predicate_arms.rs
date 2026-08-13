// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// Criterion 1: Regex arm actually tests pattern (doesn't just always fire)
#[test]
fn test_match_regex_arm_respects_pattern() {
    assert_runs_with_output(
        r#"
use system.text

let result = match "12345"
    re"^[a-z]+$": "should_not_match"
    _: "correct"
println(result)
"#,
        "correct",
    );
}

// Criterion 2: Regex arm fires when pattern matches
#[test]
fn test_match_regex_arm_fires_on_match() {
    assert_runs_with_output(
        r#"
use system.text

let result = match "12345"
    re"^\d+$": "regex_match"
    _: "other"
println(result)
"#,
        "regex_match",
    );
}

// Criterion 3: Regex arms tested in lexical order, first match wins
#[test]
fn test_match_regex_arms_lexical_order() {
    assert_runs_with_output(
        r#"
use system.text

let subject = "abc123"
let result = match subject
    re"[a-z]+": "letters"
    re"[a-z0-9]+": "alphanumeric"
    _: "other"
println(result)
"#,
        "letters",
    );
}

// Criterion 4: String literal arm fires
#[test]
fn test_match_string_literal_arm_fires() {
    assert_runs_with_output(
        r#"
let subject = "abc"
let result = match subject
    "abc": "string_match"
    _: "other"
println(result)
"#,
        "string_match",
    );
}

// Criterion 5: String literal arms discriminate correctly
#[test]
fn test_match_string_literal_arms_discriminate() {
    assert_runs_with_output(
        r#"
let subject = "def"
let result = match subject
    "abc": "wrong1"
    "def": "correct"
    "ghi": "wrong2"
    _: "other"
println(result)
"#,
        "correct",
    );
}

// Criterion 6: Mixed string and regex arms work consistently
#[test]
fn test_match_mixed_string_regex_arms() {
    assert_runs_with_output(
        r#"
use system.text

let subject = "123"
let result = match subject
    "abc": "string"
    re"\d+": "regex"
    _: "other"
println(result)
"#,
        "regex",
    );
}

// Criterion 7: Float literal arms fire on equality
#[test]
fn test_match_float_literal_arm_fires() {
    assert_runs_with_output(
        r#"
let subject = 3.14
let result = match subject
    3.14: "exact_match"
    _: "other"
println(result)
"#,
        "exact_match",
    );
}

// Criterion 8: NaN subject falls through to catch-all
#[test]
fn test_match_float_nan_falls_through() {
    assert_runs_with_output(
        r#"
fn make_nan() float
    let x = 0.0
    x / x

let nan_val = make_nan()
let result = match nan_val
    3.14: "float_match"
    _: "other"
println(result)
"#,
        "other",
    );
}

// Criterion 9: Guard on predicate arm
#[test]
fn test_match_predicate_arm_with_guard() {
    assert_runs_with_output(
        r#"
use system.text

let subject = "12345"
let subject_len = subject.length()
let result = match subject
    re"^\d+$" if subject_len > 10: "long_digits"
    re"^\d+$": "short_digits"
    _: "other"
println(result)
"#,
        "short_digits",
    );
}

// Criterion 10: Predicate arms in loop do not leak memory and give correct answers
#[test]
fn test_match_predicate_arms_loop_no_leak() {
    assert_runs_with_output(
        r#"
use system.text

var count = 0
var i = 0
while i < 10000
    let subject = "test"
    let result = match subject
        re"test": 1
        _: 0
    count = count + result
    i = i + 1
println(f"{count}")
"#,
        "10000",
    );
}

// Regression: Guard failure on predicate arm falls through to next predicate arm
#[test]
fn test_match_regex_guard_fails_falls_to_next_predicate() {
    assert_runs_with_output(
        r#"
use system.text

let subject = "test"
let n = 3
let result = match subject
    re"^t" if n > 10: "first_with_guard"
    re"^te": "second_regex"
    _: "other"
println(result)
"#,
        "second_regex",
    );
}

// Regression: Guard failure on string literal arm falls through to next predicate arm
#[test]
fn test_match_string_guard_fails_falls_to_next_predicate() {
    assert_runs_with_output(
        r#"
use system.text

let subject = "test"
let n = 3
let result = match subject
    "test" if n > 10: "first_with_guard"
    re"^te": "second_regex"
    _: "other"
println(result)
"#,
        "second_regex",
    );
}

// Regression: Subject is evaluated exactly once across predicate chain
// (we just verify the correct arm is hit; the subject-once property
// is verified by the memory-leak test above, since multiple evaluations
// would create leaks if predicate temps aren't released)
#[test]
fn test_match_predicate_chain_correctness() {
    assert_runs_with_output(
        r#"
use system.text

let subject = "test"
let result = match subject
    re"nomatch1": "wrong1"
    re"nomatch2": "wrong2"
    re"^te": "correct"
    _: "other"
println(result)
"#,
        "correct",
    );
}
