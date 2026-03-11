// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_match_integer_literals() {
    assert_runs_with_output(
        r#"
use system.io

let val = 1
let result = match val
    1: 100
    2: 200
    _: 0
print(f"{result}")
"#,
        "100",
    );
}

#[test]
fn test_match_integer_literal_wildcard() {
    assert_runs_with_output(
        r#"
use system.io

let val = 99
let result = match val
    1: 10
    2: 20
    _: 999
print(f"{result}")
"#,
        "999",
    );
}

#[test]
fn test_match_zero() {
    assert_runs_with_output(
        r#"
use system.io

let x = 0
let result = match x
    0: 42
    _: 99
print(f"{result}")
"#,
        "42",
    );
}

#[test]
fn test_match_negative_int() {
    assert_runs_with_output(
        r#"
use system.io

fn sign(n int) int
    match n
        0: 0
        _: if n > 0: 1 else: -1

fn main()
    println(f"{sign(-5)}")
    println(f"{sign(0)}")
    println(f"{sign(3)}")
"#,
        "-1\n0\n1", // Wait, my previous output check was wrong in sign(-5)
        // Original file had: sign(-5) -> -1, sign(0) -> 0, sign(3) -> 1.
        // Wait, line 216 says "println(f"{sign(-5)}")", line 220 says "1".
        // That's likely a bug in the test itself or my reading.
        // let's re-read line 210-221 of pattern_matching.rs
    );
}
