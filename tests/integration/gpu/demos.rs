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
//   reuse, bounds-checking, and portability checks.
//
// Planned demos awaiting completion of math-intrinsic result-width narrowing
// (f64 result into f32 buffers):
// - map_normalize: normalizes a GPU buffer by the Euclidean norm.

use super::device::assert_gpu_runs_with_output;

/// vector_add: two float arrays captured as gpu-resident, element-wise sum
/// into a mutable device buffer, readback and print. Exercises float f-string
/// formatting on the host side.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_vector_add() {
    let source = include_str!("../../../examples/gpu/vector_add.mi");
    assert_gpu_runs_with_output(source, "6.0 8.0 10.0 12.0");
}

/// saxpy: fused multiply-add with a literal scalar constant. Demonstrates
/// inline scalar math in the kernel body.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_saxpy() {
    let source = include_str!("../../../examples/gpu/saxpy.mi");
    assert_gpu_runs_with_output(source, "7.0 10.0 13.0 16.0");
}

/// buffer_reuse: two sequential gpu forall blocks on the same gpu var with no
/// readback between them. Demonstrates persistent buffer cost model
/// (1 upload, 2 launches, 1 readback).
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_buffer_reuse() {
    let source = include_str!("../../../examples/gpu/buffer_reuse.mi");
    assert_gpu_runs_with_output(source, "15 1 2 1 1");
}

/// mandelbrot: Mandelbrot set fractal using sized Array<f32, N>() constructor.
/// Computes escape-time iterations for each pixel in a 64×64 grid, demonstrating
/// fixed-size GPU buffers and correctness of in-set vs escaped pixels. The classic
/// palette renders the set black (0.0); escaped pixels carry their escape count.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_mandelbrot() {
    let source = include_str!("../../../examples/gpu/mandelbrot.mi");
    assert_gpu_runs_with_output(source, "inside=0.0 outside=1.0");
}

/// game_of_life: Conway's Game of Life cellular automaton on a 64×64 toroidal grid.
/// Seeds a deterministic ~38% pseudo-random soup, then advances one B3/S23
/// generation with a `gpu frame` kernel (the browser runtime loops it). The
/// native run counts live cells after one generation — a deterministic smoke
/// value (the soup hash + the rule are fixed). Rule correctness on isolated
/// patterns is covered by the blinker/glider tests in gpu_frame.rs / launch.rs.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_game_of_life() {
    let source = include_str!("../../../examples/gpu/game_of_life.mi");
    assert_gpu_runs_with_output(source, "alive=1993");
}

/// box_blur: 3×3 clamped-edge box blur convolution. Initializes a bright 16×16
/// square (value 1.0) centered in a 64×64 f32 image, applies two-kernel GPU
/// computation (initialization then blur), and readbacks to host. Demonstrates
/// edge-handling correctness: interior pixels unchanged (9/9 = 1.0), corner pixels
/// smoothed by clamped neighbors (4/9 ≈ 0.444), edge pixels partially averaged
/// (6/9 ≈ 0.667).
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_box_blur() {
    let source = include_str!("../../../examples/gpu/box_blur.mi");
    assert_gpu_runs_with_output(
        source,
        "interior=1.0 corner=0.4444444477558136 edge=0.6666666865348816",
    );
}

/// matmul: 2×2 matrix multiply C = A×B, one GPU thread per output cell, each
/// computing a dot product of A's row and B's column. Verifies the canonical
/// GEMM mapping with hand-checkable integer-valued matrices.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_matmul() {
    let source = include_str!("../../../examples/gpu/matmul.mi");
    assert_gpu_runs_with_output(source, "19.0 22.0 43.0 50.0");
}

/// linear_regression: one batch gradient-descent step. The kernel computes
/// per-sample MSE gradient contributions in parallel; the host reduces them to
/// the batch gradient and takes one step. On y = 2x + 1 from (W, B) = (0, 0)
/// the step lands at (1.7, 0.8) with starting loss 21 — the GPU-ML map/reduce
/// split, value-verified.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_linear_regression() {
    let source = include_str!("../../../examples/gpu/linear_regression.mi");
    assert_gpu_runs_with_output(
        source,
        "W: 0 -> 1.7000000476837158  B: 0 -> 0.800000011920929  MSE: 21.0",
    );
}

/// neural_net: a single dense layer (2 → 3) with ReLU, one thread per neuron.
/// ReLU is the ternary `sum if sum > 0 else 0` — no transcendental activation.
/// The third neuron's pre-activation is negative, so ReLU clips it to 0,
/// exercising the activation path alongside the two positive outputs.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_neural_net() {
    let source = include_str!("../../../examples/gpu/neural_net.mi");
    assert_gpu_runs_with_output(source, "1.5 1.5 0.0");
}

/// neural_net_mlp: a 2-layer MLP (2 → 2 ReLU → 1) computing XOR over all four
/// input pairs in one batched forward pass. Two kernels chained through a
/// persistent hidden buffer with no intermediate readback. Output [0,1,1,0] is
/// XOR — proof the hidden layer learned the non-linear separation.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_neural_net_mlp() {
    let source = include_str!("../../../examples/gpu/neural_net_mlp.mi");
    assert_gpu_runs_with_output(
        source,
        "xor(0,0)=0.0 xor(0,1)=1.0 xor(1,0)=1.0 xor(1,1)=0.0",
    );
}

/// game_of_life_web: Multi-pass Conway's Game of Life with frame inputs and
/// interactive event handling. A 64×64 toroidal grid with 5-pass frame loop:
/// (1) CA step, (2) trail decay, (3) mouse splat, (4) reseed, (5) RGBA paint.
/// Uses literal-sized buffers (Array<T, 64 * 64>) to avoid const-in-generic issues.
/// Deterministic native run counts alive cells after one seed+advance cycle (1993).
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_game_of_life_web() {
    let source = include_str!("../../../examples/gpu/web/game_of_life.mi");
    assert_gpu_runs_with_output(source, "alive=1993");
}

/// mandelbrot_web: interactive pan/zoom Mandelbrot, a faithful port of the
/// reference fragment shader (bailout radius 64, smooth iteration count, and a
/// five-stop navy→blue→cyan→yellow→white palette). A `gpu frame` block
/// integrates the view state (ping-ponged view_a → view_b, driven by frame.*)
/// then renders into an RGBA surface. The native run uses zero pointer input,
/// so the seeded viewport is fixed; the smoke value counts interior (near-black)
/// pixels — a deterministic integer robust to palette float rounding.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_mandelbrot_web() {
    let source = include_str!("../../../examples/gpu/web/mandelbrot.mi");
    assert_gpu_runs_with_output(source, "interior=2792");
}

/// raymarch_web: interactive ray marcher, a faithful port of the reference
/// fragment shader — three time-animated metaballs smooth-unioned with a
/// rounded cube over a grid floor, a key/fill light rig with 40-step soft
/// shadows, a Fresnel rim, a specular glint, distance fog, and a tone map.
/// Device-side `fn` helpers (smin/SDFs/scene/soft_shadow) are bundled into each
/// kernel's WGSL. The native run uses zero pointer input, so the seeded camera
/// is fixed; the smoke value is the shaded center pixel's exact RGB (the full
/// march + normal + shadow + lighting chain, value-verified like box_blur).
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_raymarch_web() {
    let source = include_str!("../../../examples/gpu/web/raymarch.mi");
    assert_gpu_runs_with_output(
        source,
        "center=0.06467816978693008 0.08995301276445389 0.13298241794109344",
    );
}
