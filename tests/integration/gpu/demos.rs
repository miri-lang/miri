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
//
// The `red_sum_milli` checksums are host-side reductions over the read-back
// frame, so they are stated at the width the host accumulates in. When an
// untyped float literal became f64, the `var total = 0.0` accumulators in the
// demos widened with it and the sums moved in their low digits. The device
// output did not change: pinning an accumulator back to `f32` reproduces the
// previous checksum of each demo exactly.

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
    assert_gpu_runs_with_output(source, "interior=1.0 corner=0.44444445 edge=0.6666667");
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

/// tiled_matmul: 4×4 GEMM through the explicit `gpu fn` launch surface — a 2×2
/// grid of 2×2 blocks, each block cooperatively staging tiles of A and B into
/// `shared` workgroup memory with `kernel.barrier()` between load and
/// accumulate. B is the identity, so C = A verifies both tile loads and both
/// K-iterations end-to-end.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_tiled_matmul() {
    let source = include_str!("../../../examples/gpu/tiled_matmul.mi");
    assert_gpu_runs_with_output(
        source,
        "1.0 2.0 3.0 4.0 5.0 6.0 7.0 8.0 9.0 10.0 11.0 12.0 13.0 14.0 15.0 16.0",
    );
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
    assert_gpu_runs_with_output(source, "W: 0 -> 1.7000000000000002  B: 0 -> 0.8  MSE: 21.0");
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
/// Buffer sizes derive from named `const CELLS`/`PAINT` value-generic args.
/// Deterministic native run sums the paint red channel across the frame after
/// one seed+advance cycle — an order-independent total over the Life step,
/// trail decay, and palette.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_game_of_life_web() {
    let source = include_str!("../../../examples/gpu/web/game_of_life.mi");
    assert_gpu_runs_with_output(source, "red_sum_milli=170721356");
}

/// mandelbrot_web: interactive pan/zoom Mandelbrot, a faithful port of the
/// reference fragment shader (bailout radius 64, smooth iteration count, and a
/// five-stop navy→blue→cyan→yellow→white palette). A `gpu frame` block
/// integrates the view state (ping-ponged view_a → view_b, driven by frame.*)
/// then renders into an RGBA surface. The native run uses zero pointer input,
/// so the seeded viewport is fixed; the smoke value sums the tone-mapped red
/// channel across the frame — a deterministic, order-independent total over the
/// escape-time iteration and the smooth palette.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_mandelbrot_web() {
    let source = include_str!("../../../examples/gpu/web/mandelbrot.mi");
    assert_gpu_runs_with_output(source, "red_sum_milli=335842575");
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
    assert_gpu_runs_with_output(source, "center=0.06467817 0.08995301 0.13298242");
}

/// blackhole_web: a Schwarzschild black hole rendered by integrating one photon
/// null geodesic per pixel through the orbit-plane ODE d²u/dφ² = -u + 1.5·u², so
/// the light bends around the hole, the accretion disk wraps over the top, and
/// the Einstein ring forms. Device-side `fn` helpers (hash/noise/fbm/star_layer/
/// disk_opacity) are bundled into each kernel's WGSL; the disk emission and the
/// lensed starfield background are inlined into the render kernel. The native run
/// uses zero pointer input, so the seeded camera is fixed; the smoke value is the
/// shaded center pixel's exact RGB — the full geodesic + disk + background chain,
/// value-verified like raymarch.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_blackhole_web() {
    let source = include_str!("../../../examples/gpu/web/blackhole.mi");
    assert_gpu_runs_with_output(source, "red_sum_milli=117766922");
}

/// wormhole_web: a traversable Morris–Thorne wormhole rendered by integrating one
/// photon null geodesic per pixel through the throat profile r(ℓ) = √(K²+max(0,
/// |ℓ|−A)²) with the conserved-impact-parameter ODE ℓ̈ = b²·r'/r³. Rays clearing
/// the throat (ℓ < 0) sample the far universe's cool sky, grazing rays bend back
/// into the warm home sky, and light piling at the throat forms the Einstein
/// ring. Device-side `fn` helpers (hash/noise/nebula_fbm/star_layer) are bundled
/// into each kernel's WGSL; the two skies are inlined into the render kernel. The
/// native run uses zero pointer input; the smoke value is the whole-frame red
/// channel sum — the full throat + sky-select + ring chain, value-verified.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_wormhole_web() {
    let source = include_str!("../../../examples/gpu/web/wormhole.mi");
    assert_gpu_runs_with_output(source, "red_sum_milli=106806036");
}

/// particles_web: 1,048,576 particles advected through a two-octave curl-noise
/// flow field, scattered with additive GPU atomics into a fixed-point intensity
/// surface, tone-mapped to a white/blue field. A `gpu frame` block runs four
/// buffer-disjoint passes (advect / fade / atomic-scatter / present) over
/// ping-ponged particle state and a ping-ponged `Array<Atomic<u32>, N>` surface.
/// The native run uses zero pointer input; the smoke value is the whole-surface
/// intensity total — the per-particle deposit summed over every particle, so it
/// is independent of atomic contention order and verifies the advect + atomic
/// scatter chain end to end.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_particles_web() {
    let source = include_str!("../../../examples/gpu/web/particles.mi");
    assert_gpu_runs_with_output(source, "surface_total=35753440");
}

/// fluid_web: Stam-style stable-fluids simulation, the frame-graph stress test.
/// A `gpu frame` block runs the full pressure-projection pipeline as ordered
/// passes over ping-ponged fields: splat, semi-Lagrangian advect (velocity then
/// dye, manual bilinear), divergence, a 24-iteration Jacobi pressure solve
/// (written as `for _ in 0..12` of two ping-pong passes — the repeated-pass
/// unroll), gradient subtraction, and a bilinearly-sampled tone-mapped display.
/// The native run uses zero pointer input, so an ambient swirl drives the field;
/// the smoke
/// value is the whole-field dye-green total (scaled to milli-units), a finite
/// deterministic integer that proves the chain runs without NaN blow-up.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_fluid_web() {
    let source = include_str!("../../../examples/gpu/web/fluid.mi");
    assert_gpu_runs_with_output(source, "dye_sum_milli=41066");
}

/// neural_web: the capstone — a 2-12-12-1 MLP (205 parameters) trained entirely
/// on the GPU to classify cycling 2-D datasets (spiral, rings, XOR). A `gpu
/// frame` block runs the training as buffer-disjoint passes: a single-thread
/// full-batch analytic backprop epoch that accumulates the exact gradient into
/// local scratch and applies a momentum update (wa/va -> wb/vb, ping-ponged by
/// the host), a stats pass for the HUD
/// readback, a dataset-regeneration pass, and a per-pixel decision-field render
/// with a data-point overlay. The native run does one frame (one epoch)
/// from the seeded weights; the smoke value is the post-step loss and accuracy
/// (in milli-units), proving the on-device forward + backprop + update chain
/// drives the net well below its random-init baseline.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn demo_neural_web() {
    let source = include_str!("../../../examples/gpu/web/neural.mi");
    assert_gpu_runs_with_output(source, "loss_milli=583 acc_milli=700");
}
