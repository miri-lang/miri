// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_array_index_out_of_bounds_literal() {
    assert_compiler_error(
        r#"
let a = [1, 2, 3]
let x = a[5]
    "#,
        "Array index out of bounds",
    );
}

#[test]
fn test_array_mixed_types() {
    assert_compiler_error(
        r#"
let a = [1, "hello"]
    "#,
        "Array elements must have the same type",
    );
}

#[test]
fn test_array_non_int_index() {
    assert_compiler_error(
        r#"
let a = [1, 2, 3]
let x = a["x"]
    "#,
        "Array index must be an integer",
    );
}

#[test]
fn test_array_index_out_of_bounds_runtime() {
    assert_runtime_error(
        r#"
use system.io

fn get_index() int
    5

fn main()
    let a = [1, 2, 3]
    let i = get_index()
    println(f"{a[i]}")
    "#,
        "Runtime error: Array index out of bounds",
    );
}

#[test]
fn test_array_index_assignment_out_of_bounds_runtime() {
    assert_runtime_error(
        r#"
use system.io

fn get_index() int
    5

fn main()
    var a = [1, 2, 3]
    let i = get_index()
    a[i] = 99
    println(f"{a[0]}")
    "#,
        "Runtime error: Array index out of bounds",
    );
}

#[test]
fn test_array_negative_index_runtime() {
    assert_runtime_error(
        r#"
use system.io

fn get_index() int
    -1

fn main()
    let a = [10, 20, 30]
    let i = get_index()
    println(f"{a[i]}")
    "#,
        "Runtime error: Array index out of bounds",
    );
}
