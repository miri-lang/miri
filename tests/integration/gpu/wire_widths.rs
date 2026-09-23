// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every scalar width a GPU buffer element or a captured scalar may have
//! crosses the host/device boundary under one rule: the device holds a 32-bit
//! lane (or the width the source named, for `f16`/`f32`/`f64`), and the host
//! widens or narrows each value on upload and readback. These tests read the
//! device's writes back and check the values, so a buffer declared at one
//! width on the device and marshalled at another on the host shows up as a
//! wrong number rather than passing unnoticed.

use super::helpers::compile_to_wgsl;
use super::utils::{
    assert_compiler_error, assert_runs_with_output, assert_runtime_error, assert_type_checks,
};

/// A `gpu fn` that stores `value` into every element of an
/// `Array<element, 4>` and prints all four elements after readback.
fn kernel_fill_program(element: &str, value: &str) -> String {
    format!(
        "
use system.collections.array

gpu fn k(a out Array<{element}, 4>)
    let i = kernel.global_idx.x
    if i < 4
        a[i] = {value}

fn main()
    gpu var a = Array<{element}, 4>()
    k(a).launch(Dim3(1, 1, 1), Dim3(4, 1, 1))
    let host = a
    println(f\"{{host[0]}} {{host[1]}} {{host[2]}} {{host[3]}}\")
"
    )
}

/// A `forall` that adds the captured scalar `k` (declared as `let k {ty} = {value}`)
/// to each index of an int buffer and prints the last element.
fn scalar_capture_program(ty: &str, value: &str) -> String {
    format!(
        "
use system.gpu

fn main()
    gpu var buf = [0, 0, 0, 0]
    let k {ty} = {value}
    gpu forall i in 0..4
        buf[i] = i + k
    let host = buf
    println(f\"{{host[3]}}\")
"
    )
}

/// A `forall` that stores the captured unsigned scalar `k` into every element
/// of a buffer of the same type and prints the last element.
fn unsigned_capture_program(ty: &str, value: &str) -> String {
    format!(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var buf = Array<{ty}, 4>()
    let k {ty} = {value}
    gpu forall i in 0..4
        buf[i] = k
    let host = buf
    println(f\"{{host[3]}}\")
"
    )
}

fn assert_fill_round_trips(element: &str, value: &str) {
    let expected = format!("{value} {value} {value} {value}");
    assert_runs_with_output(&kernel_fill_program(element, value), &expected);
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i8_buffer_elements_round_trip() {
    assert_fill_round_trips("i8", "-5");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_u8_buffer_elements_round_trip() {
    assert_fill_round_trips("u8", "200");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i16_buffer_elements_round_trip() {
    assert_fill_round_trips("i16", "-300");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_u16_buffer_elements_round_trip() {
    assert_fill_round_trips("u16", "60000");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i64_buffer_elements_round_trip() {
    assert_fill_round_trips("i64", "-9");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_u64_buffer_elements_round_trip() {
    assert_fill_round_trips("u64", "9");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_float_buffer_elements_round_trip() {
    assert_fill_round_trips("float", "2.5");
}

/// The host bytes of every sub-word element must reach the device as its own
/// lane: each element starts at a distinct value and the kernel squares it in
/// place. Squaring is not byte-separable, so two `i16` elements sharing one
/// 32-bit lane cannot produce the expected values by accident.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i16_buffer_uploads_every_element() {
    let code = "
use system.gpu
use system.collections.array

fn main()
    gpu var a = Array<i16, 4>()
    a = [1 as i16, -2 as i16, 3 as i16, -4 as i16]
    gpu forall i in 0..4
        a[i] = a[i] * a[i]
    let host = a
    println(f\"{host[0]} {host[1]} {host[2]} {host[3]}\")
";
    assert_runs_with_output(code, "1 4 9 16");
}

/// An `i64` buffer captured by a `forall` travels at the same device width
/// as one passed to a `gpu fn`.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i64_forall_buffer_round_trips() {
    let code = "
use system.gpu
use system.collections.array

fn main()
    gpu var a = Array<i64, 4>()
    gpu forall i in 0..4
        a[i] = 9
    let host = a
    println(f\"{host[0]} {host[1]} {host[2]} {host[3]}\")
";
    assert_runs_with_output(code, "9 9 9 9");
}

/// Reassigning a gpu binding before any launch captures it must still marshal
/// the new host values at the device width.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_int_buffer_reassigned_before_first_launch_round_trips() {
    let code = "
use system.gpu
use system.collections.array

fn main()
    gpu var g = [1, 2, 3]
    g = [4, 5, 6]
    gpu forall i in 0..3
        g[i] = g[i] + 1
    let host = g
    println(f\"{host[0]} {host[1]} {host[2]}\")
";
    assert_runs_with_output(code, "5 6 7");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_u64_buffer_value_above_u32_range_is_a_runtime_error() {
    let code = "
use system.gpu
use system.collections.array

fn big() u64
    return 4294967296 as u64

fn main()
    gpu var a = Array<u64, 1>()
    a = [big()]
    gpu forall i in 0..1
        a[i] = a[i] + 1
    let host = a
    println(f\"{host[0]}\")
";
    assert_runtime_error(code, "exceeds u32 range");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i32_scalar_capture_is_passed_to_the_kernel() {
    assert_runs_with_output(&scalar_capture_program("i32", "5"), "8");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_sub_word_signed_scalar_captures_are_passed_to_the_kernel() {
    assert_runs_with_output(&scalar_capture_program("i8", "-5"), "-2");
    assert_runs_with_output(&scalar_capture_program("i16", "-300"), "-297");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_unsigned_scalar_captures_are_passed_to_the_kernel() {
    assert_runs_with_output(&unsigned_capture_program("u8", "200"), "200");
    assert_runs_with_output(&unsigned_capture_program("u16", "60000"), "60000");
    assert_runs_with_output(&unsigned_capture_program("u32", "4000000000"), "4000000000");
    assert_runs_with_output(&unsigned_capture_program("u64", "7"), "7");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i64_scalar_capture_is_passed_to_the_kernel() {
    assert_runs_with_output(&scalar_capture_program("i64", "-7"), "-4");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_int_scalar_capture_above_i32_range_is_a_runtime_error() {
    let code = "
use system.gpu

fn big() int
    return 5000000000

fn main()
    gpu var buf = [0, 0, 0, 0]
    let k = big()
    gpu forall i in 0..4
        buf[i] = k
    let host = buf
    println(f\"{host[3]}\")
";
    assert_runtime_error(code, "exceeds i32 range");
}

#[test]
fn test_gpu_int_scalar_capture_of_a_constant_above_i32_range_is_a_compile_error() {
    let code = "
use system.gpu

fn main()
    gpu var buf = [0, 0, 0, 0]
    let k = 5000000000
    gpu forall i in 0..4
        buf[i] = k
";
    assert_compiler_error(code, "exceeds i32 range");
}

#[test]
fn test_gpu_f64_scalar_capture_is_a_compile_error() {
    let code = "
use system.gpu

fn main()
    gpu var buf = Array<f32, 4>()
    let s f64 = 1.5
    gpu forall i in 0..4
        buf[i] = s as f32
";
    assert_compiler_error(code, "unsupported gpu scalar capture type");
}

#[test]
fn test_gpu_i128_scalar_capture_is_a_compile_error() {
    let code = "
use system.gpu

fn main()
    gpu var buf = [0, 0, 0, 0]
    let k i128 = 1
    gpu forall i in 0..4
        buf[i] = k as int
";
    assert_compiler_error(code, "unsupported gpu scalar capture type");
}

#[test]
fn test_gpu_sub_word_and_64_bit_scalar_captures_type_check() {
    for ty in ["i8", "i16", "i32", "i64"] {
        assert_type_checks(&scalar_capture_program(ty, "1"));
    }
    for ty in ["u8", "u16", "u32", "u64"] {
        assert_type_checks(&unsigned_capture_program(ty, "1"));
    }
}

#[test]
fn test_wgsl_i64_buffer_is_declared_at_the_i32_device_width() {
    let wgsl = compile_to_wgsl(&kernel_fill_program("i64", "9"));
    assert!(wgsl.contains("array<i32>"), "{wgsl}");
    assert!(!wgsl.contains("i64"), "{wgsl}");
    assert!(!wgsl.contains("9li"), "{wgsl}");
}

#[test]
fn test_wgsl_u64_buffer_is_declared_at_the_u32_device_width() {
    let wgsl = compile_to_wgsl(&kernel_fill_program("u64", "9"));
    assert!(wgsl.contains("array<u32>"), "{wgsl}");
    assert!(!wgsl.contains("u64"), "{wgsl}");
}

#[test]
fn test_wgsl_i32_scalar_capture_is_a_32_bit_input_field() {
    let wgsl = compile_to_wgsl(&scalar_capture_program("i32", "5"));
    assert!(wgsl.contains("f0: i32"), "{wgsl}");
}

#[test]
fn test_wgsl_sub_word_scalar_captures_widen_to_32_bit_input_fields() {
    let signed = compile_to_wgsl(&scalar_capture_program("i16", "5"));
    assert!(signed.contains("f0: i32"), "{signed}");
    let unsigned = compile_to_wgsl(&unsigned_capture_program("u8", "5"));
    assert!(unsigned.contains("f0: u32"), "{unsigned}");
}

/// A `List` has no fixed device layout, so a kernel cannot bind one as a
/// storage buffer; the capture is refused at type check, with the fix named,
/// rather than failing inside the WGSL backend.
#[test]
fn test_gpu_forall_capture_of_a_list_is_a_compile_error() {
    for element in ["i64", "f32"] {
        let code = format!(
            "
use system.gpu
use system.collections.list

fn main()
    let src = List<{element}>()
    gpu var buf = src
    gpu forall i in 0..2
        buf[i] = buf[i] + buf[i]
"
        );
        assert_compiler_error(&code, "no fixed device layout");
    }
}
