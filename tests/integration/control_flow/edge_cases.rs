// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_while_zero_iterations() {
    assert_runs_with_output(
        r#"
use system.io
var x = 99
while false
    x = 0
print(f"{x}")
    "#,
        "99",
    );
}

#[test]
fn test_for_empty_range() {
    assert_runs_with_output(
        r#"
use system.io
var count = 0
for i in 5..5
    count = count + 1
print(f"{count}")
    "#,
        "0", // Span::new(5, 5) is empty (exclusive)
    );
}

#[test]
fn test_deeply_nested_if_else() {
    assert_runs_with_output(
        r#"
use system.io
let x = 7
let r = if x < 0
    -1
else if x == 0
    0
else if x < 5
    1
else if x < 10
    2
else
    3
print(f"{r}")
    "#,
        "2",
    );
}
