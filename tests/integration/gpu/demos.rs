// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// GPU demo programs: production-grade examples of the residency surface in
// action. These are the public showcase of GPU capabilities — they live in
// `examples/gpu/` as the single source of truth, loaded here via
// `include_str!` for CI verification.
//
// Each demo tests:
// - Compilation succeeds (adapter-less CI still runs).
// - Value correctness (adapter-present CI asserts exact output).
// - Surface compliance: residency keywords, cost-class ordering, buffer
//   reuse, bounds-checking, and portability checks per §17.
//
// Deferred demos (compiler bugs, not scope for this task):
// - map_normalize: math-intrinsic result temps typed f64 while f32 buffers → width
//   mismatch → zeros on Metal (blocker: F23).

use super::device::assert_gpu_runs_with_output;

/// vector_add: two float arrays captured as gpu-resident, element-wise sum
/// into a mutable device buffer, readback and print. Exercises float f-string
/// formatting on the host side.
#[test]
fn demo_vector_add() {
    let source = include_str!("../../../examples/gpu/vector_add.mi");
    assert_gpu_runs_with_output(source, "6.0 8.0 10.0 12.0");
}

/// saxpy: fused multiply-add with a literal scalar constant. Demonstrates
/// inline scalar math in the kernel body.
#[test]
fn demo_saxpy() {
    let source = include_str!("../../../examples/gpu/saxpy.mi");
    assert_gpu_runs_with_output(source, "7.0 10.0 13.0 16.0");
}

/// buffer_reuse: two sequential gpu for blocks on the same gpu var with no
/// readback between them. Demonstrates persistent buffer cost model
/// (1 upload, 2 launches, 1 readback).
#[test]
fn demo_buffer_reuse() {
    let source = include_str!("../../../examples/gpu/buffer_reuse.mi");
    assert_gpu_runs_with_output(source, "15 1 2 1 1");
}
