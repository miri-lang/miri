// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_gpu::context::GpuError;
use miri_runtime_gpu::launch::*;
use wgpu::Features;

#[test]
fn readback_with_null_array_fails() {
    let result = unsafe { miri_gpu_readback(1, std::ptr::null()) };
    assert_eq!(result, 0);
}

#[test]
fn scalar_scan_matches_whole_tokens_only() {
    assert!(wgsl_uses_scalar("var<storage> a: array<i64>;", "i64"));
    assert!(wgsl_uses_scalar("let x = f64(1.0);", "f64"));
    assert!(!wgsl_uses_scalar("var v: vec4<f32>;", "f64"));
    // A 64-bit keyword embedded in a longer identifier is not a match.
    assert!(!wgsl_uses_scalar("var xi64y: i32;", "i64"));
    assert!(!wgsl_uses_scalar("var i64x: i32;", "i64"));
}

#[test]
fn i64_kernel_refused_without_shader_int64() {
    let wgsl = "@group(0) @binding(0) var<storage, read_write> a: array<i64>;";
    let err = check_required_shader_features(wgsl, Features::empty())
        .expect_err("i64 kernel must be refused when SHADER_INT64 is absent");
    assert!(matches!(err, GpuError::UnsupportedScalar(_)));
}

#[test]
fn f64_kernel_refused_without_shader_f64() {
    let wgsl = "@group(0) @binding(0) var<storage, read_write> a: array<f64>;";
    let err = check_required_shader_features(wgsl, Features::SHADER_INT64)
        .expect_err("f64 kernel must be refused when SHADER_F64 is absent");
    assert!(matches!(err, GpuError::UnsupportedScalar(_)));
}

#[test]
fn scalar_kernel_passes_when_features_present() {
    let wgsl = "var<storage> a: array<i64>; var<storage> b: array<f64>;";
    let enabled = Features::SHADER_INT64 | Features::SHADER_F64;
    assert!(check_required_shader_features(wgsl, enabled).is_ok());
}

#[test]
fn i32_kernel_needs_no_64bit_feature() {
    let wgsl = "@group(0) @binding(0) var<storage, read_write> a: array<i32>;";
    assert!(check_required_shader_features(wgsl, Features::empty()).is_ok());
}

#[test]
fn readback_of_unbacked_handle_is_a_noop_success() {
    // A handle never captured by a launch has no resident buffer; the
    // host array is already authoritative, so the readback succeeds
    // without touching the device.
    let mut bytes = [0u8; 16];
    let header = MiriArrayHeader {
        data: bytes.as_mut_ptr(),
        elem_count: 4,
        elem_size: 4,
    };
    let result = unsafe { miri_gpu_readback(u64::MAX, &header) };
    assert_eq!(result, 1);
}
