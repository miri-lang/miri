// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_match_bool_true() {
    assert_runs_with_output(
        r#"

let b = true
let result = match b
    true: 1
    false: 0
print(f"{result}")
"#,
        "1",
    );
}

#[test]
fn test_match_bool_false() {
    assert_runs_with_output(
        r#"

let b = false
let result = match b
    true: 1
    false: 0
print(f"{result}")
"#,
        "0",
    );
}
