// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Device integers are 32-bit. An integer literal in kernel code is judged
//! against the 32-bit lane its type occupies on the device (signed or
//! unsigned), and kernel code may not spell a 64-bit integer type, whose
//! arithmetic the device could only carry out at 32 bits.

use super::utils::{type_checker_error_test, type_checker_error_with_help_test, type_checker_test};

/// A `gpu forall` over a buffer of `element` whose body is `body` (one line,
/// indented under the loop).
fn forall_program(element: &str, body: &str) -> String {
    format!(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = Array<{element}, 2>()
    gpu forall i in 0..2
        {body}
"
    )
}

#[test]
fn test_gpu_u32_literal_at_the_top_of_the_unsigned_lane_type_checks() {
    type_checker_test(&forall_program(
        "u32",
        "let y u32 = 4294967295\n        dst[i] = y",
    ));
}

/// A literal assigned to an element takes the element's width, as it does in a
/// declaration, so a `u32` element accepts the top of the unsigned lane.
#[test]
fn test_gpu_untyped_literal_assigned_to_a_u32_element_takes_the_unsigned_lane() {
    type_checker_test(&forall_program("u32", "dst[i] = 4294967295"));
}

/// The unsigned lane still ends where `u32` does.
#[test]
fn test_gpu_literal_assigned_to_a_u32_element_past_the_lane_is_refused() {
    type_checker_error_test(
        &forall_program("u32", "dst[i] = 4294967296"),
        "Integer literal '4294967296' is out of range for GPU 32-bit unsigned integer",
    );
}

#[test]
fn test_gpu_u32_literal_past_the_unsigned_lane_is_refused() {
    type_checker_error_test(
        &forall_program("u32", "let y u32 = 4294967296\n        dst[i] = y"),
        "Integer literal '4294967296' is out of range for GPU 32-bit unsigned integer (u32 range is 0 to 4294967295)",
    );
}

#[test]
fn test_gpu_u64_typed_literal_past_the_unsigned_lane_is_refused() {
    type_checker_error_test(
        &forall_program("u64", "let y u64 = 4294967296\n        dst[i] = y"),
        "Integer literal '4294967296' is out of range for GPU 32-bit unsigned integer",
    );
}

#[test]
fn test_gpu_int_literal_past_the_signed_lane_is_still_refused() {
    type_checker_error_test(
        &forall_program("int", "dst[i] = 2147483648"),
        "Integer literal '2147483648' is out of range for GPU 32-bit signed integer (i32 range is -2147483648 to 2147483647)",
    );
}

#[test]
fn test_gpu_negated_int_literal_at_the_bottom_of_the_signed_lane_type_checks() {
    type_checker_test(&forall_program("int", "dst[i] = -2147483648"));
}

#[test]
fn test_gpu_explicit_i64_local_in_forall_is_refused() {
    type_checker_error_with_help_test(
        &forall_program("int", "var x i64 = 2000000000\n        dst[i] = x"),
        "'i64' is not supported in device code: device integers are 32-bit",
        "i32",
    );
}

#[test]
fn test_gpu_explicit_u64_local_in_gpu_fn_is_refused() {
    type_checker_error_test(
        "
use system.collections.array

gpu fn k(a out Array<u32, 4>)
    let i = kernel.global_idx.x
    let wide u64 = 7
    a[i] = 1
",
        "'u64' is not supported in device code: device integers are 32-bit",
    );
}

#[test]
fn test_gpu_explicit_i64_scalar_parameter_is_refused() {
    type_checker_error_test(
        "
use system.collections.array

gpu fn k(n i64, a out Array<int, 4>)
    let i = kernel.global_idx.x
    a[i] = 1
",
        "'i64' is not supported in device code: device integers are 32-bit",
    );
}

#[test]
fn test_gpu_cast_to_u64_is_refused() {
    type_checker_error_test(
        &forall_program("u64", "dst[i] = 7 as u64"),
        "'u64' is not supported in device code: device integers are 32-bit",
    );
}

#[test]
fn test_gpu_cast_to_i64_in_gpu_for_is_refused() {
    type_checker_error_test(
        &forall_program("int", "dst[i] = (i as i64) * 4"),
        "'i64' is not supported in device code: device integers are 32-bit",
    );
}

#[test]
fn test_gpu_64_bit_buffers_stay_usable_in_kernel_code() {
    type_checker_test(&forall_program("i64", "dst[i] = dst[i] * 2 + i"));
    type_checker_test(&forall_program("u64", "dst[i] = dst[i] + 1"));
    type_checker_test(&forall_program(
        "i64",
        "let v = dst[i]\n        dst[i] = v - 1",
    ));
    type_checker_test(
        "
use system.collections.array

gpu fn k(a out Array<i64, 4>)
    let i = kernel.global_idx.x
    a[i] = a[i] + 1
",
    );
}

#[test]
fn test_gpu_64_bit_scalar_capture_stays_usable_in_kernel_code() {
    type_checker_test(
        "
use system.gpu
use system.collections.array

fn main()
    let k i64 = 5
    gpu var dst = Array<int, 2>()
    gpu forall i in 0..2
        dst[i] = i + k
",
    );
}

#[test]
fn test_host_i64_local_beside_a_kernel_is_not_refused() {
    type_checker_test(
        "
use system.gpu
use system.collections.array

fn main()
    var total i64 = 8000000000
    gpu var dst = Array<int, 2>()
    gpu forall i in 0..2
        dst[i] = i
    total = total * 2
",
    );
}

#[test]
fn test_gpu_int_local_stays_allowed_in_kernel_code() {
    type_checker_test(&forall_program(
        "int",
        "var x int = 5\n        dst[i] = x * 2",
    ));
}
