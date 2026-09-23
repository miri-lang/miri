// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Kernel integer arithmetic at the width the source named.
//!
//! The device computes every integer in a 32-bit lane, so an 8- or 16-bit
//! result has to be brought back to its own width before anything reads it —
//! otherwise an overflow the host wraps survives on the device and a later
//! division or comparison sees a different number. Each program here computes
//! the same expression on the host and in a kernel and prints both, so a
//! device that disagrees with the host prints a mismatched pair.

use super::helpers::compile_to_wgsl;
use super::utils::{assert_compiler_error, assert_runs_with_output};

/// A `forall` that stores `kernel_expression` (over the buffer element
/// `buf[i]`) back into a two-element buffer of `element` seeded with `seed`,
/// then prints the device result beside `host_expression` computed over a
/// host `h` holding the same seed.
fn narrow_program(
    element: &str,
    seed: &str,
    kernel_expression: &str,
    host_expression: &str,
) -> String {
    format!(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var buf = [{seed} as {element}, {seed} as {element}]
    gpu forall i in 0..2
        buf[i] = {kernel_expression}
    let device = buf
    let h {element} = {seed}
    let host {element} = {host_expression}
    println(f\"{{device[1]}} {{host}}\")
"
    )
}

fn assert_device_matches_host(program: &str, expected: &str) {
    assert_runs_with_output(program, &format!("{expected} {expected}"));
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_u8_add_wraps_at_eight_bits() {
    let program = narrow_program("u8", "10", "buf[i] + 250 as u8", "h + 250 as u8");
    assert_device_matches_host(&program, "4");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_u8_divide_after_overflow_sees_the_wrapped_value() {
    let program = narrow_program(
        "u8",
        "10",
        "(buf[i] + 250 as u8) / 2 as u8",
        "(h + 250 as u8) / 2 as u8",
    );
    assert_device_matches_host(&program, "2");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_u8_compare_after_overflow_sees_the_wrapped_value() {
    let program = narrow_program(
        "u8",
        "10",
        "1 as u8 if buf[i] + 250 as u8 < 10 as u8 else 0 as u8",
        "1 as u8 if h + 250 as u8 < 10 as u8 else 0 as u8",
    );
    assert_device_matches_host(&program, "1");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i8_add_wraps_to_a_negative_value() {
    let program = narrow_program("i8", "100", "buf[i] + 100 as i8", "h + 100 as i8");
    assert_device_matches_host(&program, "-56");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i8_subtract_wraps_to_a_positive_value() {
    let program = narrow_program("i8", "-100", "buf[i] - 100 as i8", "h - 100 as i8");
    assert_device_matches_host(&program, "56");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i16_multiply_wraps_at_sixteen_bits() {
    let program = narrow_program("i16", "300", "buf[i] * 300 as i16", "h * 300 as i16");
    assert_device_matches_host(&program, "24464");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_u16_multiply_then_modulo_sees_the_wrapped_value() {
    let program = narrow_program(
        "u16",
        "300",
        "(buf[i] * 300 as u16) % 1000 as u16",
        "(h * 300 as u16) % 1000 as u16",
    );
    assert_device_matches_host(&program, "464");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_cast_into_u8_truncates_to_eight_bits() {
    let program = narrow_program("u8", "0", "(i + 300) as u8", "(1 + 300) as u8");
    assert_device_matches_host(&program, "45");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_cast_into_i8_truncates_and_sign_extends() {
    let program = narrow_program("i8", "0", "(i + 199) as i8", "(1 + 199) as i8");
    assert_device_matches_host(&program, "-56");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_u32_literal_at_the_top_of_the_lane_reaches_the_device() {
    let code = "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = Array<u32, 2>()
    gpu forall i in 0..2
        let y u32 = 4294967295
        dst[i] = y
    let host = dst
    println(f\"{host[1]}\")
";
    assert_runs_with_output(code, "4294967295");
}

#[test]
fn test_wgsl_u8_arithmetic_is_brought_back_to_eight_bits() {
    let wgsl = compile_to_wgsl(&narrow_program("u8", "10", "buf[i] + 250 as u8", "h"));
    assert!(
        wgsl.contains("extractBits((buf[i32(_9)] + _11), 0u, 8u)"),
        "{wgsl}"
    );
}

#[test]
fn test_wgsl_i16_arithmetic_and_cast_are_brought_back_to_sixteen_bits() {
    let wgsl = compile_to_wgsl(&narrow_program("i16", "300", "buf[i] * 300 as i16", "h"));
    assert!(wgsl.contains("extractBits(i32(300), 0u, 16u)"), "{wgsl}");
    assert!(
        wgsl.contains("extractBits((buf[i32(_9)] * _11), 0u, 16u)"),
        "{wgsl}"
    );
}

#[test]
fn test_wgsl_int_arithmetic_is_not_renormalized() {
    let wgsl = compile_to_wgsl(&narrow_program("int", "10", "buf[i] + 250", "h"));
    assert!(!wgsl.contains("extractBits"), "{wgsl}");
}

/// The device would carry out this `i64` arithmetic at 32 bits and store
/// -589934 where the host computes 8000000, so the explicit 64-bit local is
/// refused instead.
#[test]
fn test_gpu_explicit_i64_arithmetic_in_forall_is_a_compile_error() {
    let code = "
use system.gpu

fn main()
    gpu var dst = [0 as i64, 0 as i64]
    gpu forall i in 0..2
        var x i64 = 2000000000
        x = x * 4
        dst[i] = x / 1000
    let host = dst
    println(f\"{host[0]}\")
";
    assert_compiler_error(code, "'i64' is not supported in device code");
}

#[test]
fn test_gpu_u64_literal_past_the_unsigned_lane_is_a_compile_error() {
    let code = "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = Array<u32, 2>()
    gpu forall i in 0..2
        let y u32 = 4294967296
        dst[i] = y
";
    assert_compiler_error(code, "out of range for GPU 32-bit unsigned integer");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i64_buffer_arithmetic_on_the_device_matches_the_host() {
    let program = narrow_program("i64", "-7", "buf[i] * 3 + 1", "h * 3 + 1");
    assert_device_matches_host(&program, "-20");
}
