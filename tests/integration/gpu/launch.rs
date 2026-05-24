// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Native `gpu for` dispatch tests. These exercise the full compiler-driven
// pipeline: MIR `TerminatorKind::GpuLaunch` → Cranelift translation →
// marshal captures → wgpu dispatch via `miri_gpu_launch_inline` →
// readback.
//
// Owns end-to-end value correctness for `gpu for` kernels: the WGSL
// scalar mapping aligns host and device widths (`int` → `i64`, `float`
// → `f64`) so reads/writes round-trip through device memory cleanly.

use super::utils::*;

/// Smoke test verifying the infrastructure layer is wired end-to-end:
/// compilation succeeds, the binary links against `libmiri_runtime_gpu.a`,
/// and the dispatch call into `miri_gpu_launch_inline` returns.
#[test]
fn gpu_for_dispatch_does_not_crash() {
    assert_runs(
        "
use system.io
use system.gpu
use system.collections.array

let a = [1, 2, 3, 4]
let b = [10, 20, 30, 40]
var dst = [0, 0, 0, 0]
gpu for i in 0..4
    dst[i] = a[i] + b[i]
println(\"dispatched\")
",
    );
}

/// End-to-end value-correctness check for `int` (host i64 / WGSL i64):
/// element-wise add of two captured arrays into a writable destination.
/// Reads back the host buffer and prints it to compare against the
/// expected layout.
#[test]
fn vector_add_int_round_trips_through_device() {
    assert_runs_with_output(
        "
use system.io
use system.gpu
use system.collections.array

let a = [1, 2, 3, 4]
let b = [10, 20, 30, 40]
var dst = [0, 0, 0, 0]
gpu for i in 0..4
    dst[i] = a[i] + b[i]
println(f'{dst[0]} {dst[1]} {dst[2]} {dst[3]}')
",
        "11 22 33 44",
    );
}

/// End-to-end value-correctness check for scalar multiply: every element
/// of the captured `src` array is multiplied by a literal constant and
/// written to `dst`.
#[test]
fn scalar_multiply_int_round_trips_through_device() {
    assert_runs_with_output(
        "
use system.io
use system.gpu
use system.collections.array

let src = [1, 2, 3, 4, 5, 6, 7, 8]
var dst = [0, 0, 0, 0, 0, 0, 0, 0]
gpu for i in 0..8
    dst[i] = src[i] * 7
println(f'{dst[0]} {dst[7]}')
",
        "7 56",
    );
}
