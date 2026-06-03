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
fn test_gpu_for_accepts_variable_range_bound() {
    // F1 feature: variable bounds are now accepted (lowered to uniform buffers).
    type_checker_test(
        r#"
let n = 4
gpu for i in 0..n
    let x = i + 1
"#,
    );
}

#[test]
fn test_gpu_for_rejects_non_int_range_bound() {
    // The range end must be Int.
    type_checker_error_test(
        r#"
let s = "hello"
gpu for i in 0..s
    let x = i
"#,
        "must be Int",
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

#[test]
fn test_gpu_for_rejects_bool_buffer_capture() {
    type_checker_error_test(
        r#"
use system.gpu
use system.collections.array

gpu var flags = [true, false, true, false]
gpu for i in 0..4
    flags[i] = not flags[i]
"#,
        "bool",
    );
}

#[test]
fn test_gpu_for_bool_buffer_diagnostic_cites_storage_buffer() {
    type_checker_error_test(
        r#"
use system.gpu
use system.collections.array

gpu var flags = [true, false, true, false]
gpu for i in 0..4
    flags[i] = not flags[i]
"#,
        "storage buffer",
    );
}

#[test]
fn test_gpu_for_rejects_string_buffer_capture() {
    type_checker_error_test(
        r#"
use system.gpu
use system.collections.array

gpu var labels = ["a", "b", "c", "d"]
gpu for i in 0..4
    let _ = labels[i]
"#,
        "not a valid WGSL storage-buffer element",
    );
}

#[test]
fn test_gpu_for_accepts_int_buffer_capture() {
    type_checker_test(
        r#"
use system.gpu
use system.collections.array

gpu var dst = [0, 0, 0, 0]
gpu let src = [1, 2, 3, 4]
gpu for i in 0..4
    dst[i] = src[i] * 2
"#,
    );
}

#[test]
fn test_gpu_for_accepts_f32_buffer_capture() {
    type_checker_test(
        r#"
use system.gpu
use system.collections.array

gpu var dst = [0.0, 0.0, 0.0, 0.0]
gpu let src = [1.0, 2.0, 3.0, 4.0]
gpu for i in 0..4
    dst[i] = src[i] * 2.0
"#,
    );
}

#[test]
fn test_gpu_for_accepts_f64_buffer_capture() {
    type_checker_test(
        r#"
use system.gpu
use system.collections.array

gpu var dst = [3.141592653589793, 2.718281828459045, 1.4142135623730951, 0.5772156649015329]
gpu let src = [3.141592653589793, 2.718281828459045, 1.4142135623730951, 0.5772156649015329]
gpu for i in 0..4
    dst[i] = src[i] * 2.0
"#,
    );
}

#[test]
fn test_gpu_for_rejects_bool_buffer_captured_inside_if_branch() {
    type_checker_error_test(
        r#"
use system.gpu
use system.collections.array

gpu var flags = [true, false, true, false]
gpu for i in 0..4
    if i > 0
        flags[i] = not flags[i]
"#,
        "not a valid WGSL storage-buffer element",
    );
}

#[test]
fn test_gpu_for_diagnostic_message_lists_numeric_scalars() {
    type_checker_error_test(
        r#"
use system.gpu
use system.collections.array

gpu var labels = ["a", "b", "c", "d"]
gpu for i in 0..4
    let _ = labels[i]
"#,
        "numeric scalar (i32 / u32 / i64 / u64 / f32 / f64)",
    );
}
