// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{type_checker_error_test, type_checker_test};

#[test]
fn test_gpu_for_basic_accepts_numeric_body() {
    type_checker_test(
        r#"
gpu for i in 0..4
    let x = i + 1
"#,
    );
}

#[test]
fn test_gpu_for_rejects_variable_range_bound() {
    type_checker_error_test(
        r#"
let n = 4
gpu for i in 0..n
    let x = i + 1
"#,
        "Int-literal range bounds",
    );
}

#[test]
fn test_gpu_for_rejects_print_in_body() {
    type_checker_error_test(
        r#"
use system.io

gpu for i in 0..4
    print("hi")
"#,
        "not GPU-compatible",
    );
}

#[test]
fn test_gpu_for_rejects_string_local_in_body() {
    type_checker_error_test(
        r#"
gpu for i in 0..4
    let s = "x"
"#,
        "not GPU-compatible",
    );
}

#[test]
fn test_gpu_for_rejects_non_range_iterable() {
    type_checker_error_test(
        r#"
let xs = [1, 2, 3]
gpu for i in xs
    let y = i
"#,
        "bounded numeric range",
    );
}

#[test]
fn test_gpu_for_inclusive_range_accepted() {
    type_checker_test(
        r#"
gpu for i in 0..=3
    let x = i + 1
"#,
    );
}

#[test]
fn test_gpu_for_rejects_break_in_body() {
    type_checker_error_test(
        r#"
gpu for i in 0..4
    break
"#,
        "'break' is not supported inside a 'gpu for' body",
    );
}

#[test]
fn test_gpu_for_rejects_continue_in_body() {
    type_checker_error_test(
        r#"
gpu for i in 0..4
    continue
"#,
        "'continue' is not supported inside a 'gpu for' body",
    );
}

#[test]
fn test_gpu_for_permits_break_in_nested_cpu_for() {
    type_checker_test(
        r#"
gpu for i in 0..4
    for j in 0..i
        if j > 0
            break
"#,
    );
}
