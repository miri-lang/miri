// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_match_variable() {
    assert_runs_with_output(
        r#"
use system.io

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
use system.io

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
use system.io

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
use system.io

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
use system.io

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
use system.io

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
