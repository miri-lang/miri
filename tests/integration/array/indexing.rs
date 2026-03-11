// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_array_indexing() {
    assert_runs_with_output(
        r#"
use system.io

let a = [10, 20, 30]
print(f"{a[1]}")
    "#,
        "20",
    );
}

#[test]
fn test_array_first_element() {
    assert_runs_with_output(
        r#"
use system.io

let a = [1, 2, 3]
print(f"{a[0]}")
    "#,
        "1",
    );
}

#[test]
fn test_array_last_element() {
    assert_runs_with_output(
        r#"
use system.io

let a = [1, 2, 3]
print(f"{a[2]}")
    "#,
        "3",
    );
}

#[test]
fn test_array_variable_index() {
    assert_runs_with_output(
        r#"
use system.io

let i = 1
let a = [10, 20, 30]
print(f"{a[i]}")
    "#,
        "20",
    );
}

#[test]
fn test_array_index_assignment() {
    assert_runs_with_output(
        r#"
use system.io

var a = [10, 20, 30]
a[1] = 99
print(f"{a[1]}")
    "#,
        "99",
    );
}

#[test]
fn test_array_fstring() {
    assert_runs_with_output(
        r#"
use system.io

print(f"{[1, 2, 3][0]}")
    "#,
        "1",
    );
}
