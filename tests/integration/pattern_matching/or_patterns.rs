// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_match_or_patterns_first() {
    assert_runs_with_output(
        r#"
use system.io

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
use system.io

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
use system.io

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
