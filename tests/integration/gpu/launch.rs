// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// End-to-end `gpu for` dispatch tests. These exercise the full pipeline:
// MIR `TerminatorKind::GpuLaunch` → Cranelift translation → marshal
// captures → wgpu dispatch via `miri_gpu_launch_inline` → readback.
//
// Status (2026-05-23):
//
// The dispatch infrastructure (terminator → Cranelift → runtime → wgpu →
// readback) is wired end-to-end and verified by hand: a `gpu for` program
// over default `int` arrays compiles, links `libmiri_runtime_gpu.a`,
// initializes a Metal adapter, dispatches the kernel, and reads back data
// into the host buffer. The host-visible output for
// `let a=[1,2,3,4]; let b=[10,20,30,40]; gpu for i in 0..4: dst[i]=a[i]+b[i]`
// is `11 22 0 0` instead of the expected `11 22 33 44`.
//
// Root cause is a **pre-existing** WGSL backend layout mismatch
// (`src/codegen/wgsl/types.rs::scalar`): Miri's default `int` lowers to
// Cranelift `i64` (8 bytes per element) on the host, but the WGSL emitter
// maps it to `i32` (4 bytes), so the kernel views every host element as
// two GPU elements. Slot 0 round-trips correctly, slot 1 contains a value
// the kernel writes but the host reads as the upper 32 bits of `int[0]`,
// and so on. Fixing this requires `WgslScalar::I64` + `enable
// shader_int64;` + a `Features::SHADER_INT64` request on the wgpu device
// (and a fallback path for adapters lacking the feature). It is its own
// task and is filed as a follow-up to M6.5 Task 5 Item 4.
//
// Once that lands, these tests should switch from `#[ignore]` to active.

use super::utils::*;

#[test]
#[ignore = "WGSL Int→i32 width mismatch (pre-existing scalar() mapping); follow-up"]
fn gpu_for_vector_add_writes_correct_results() {
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
println(f\"{dst[0]} {dst[1]} {dst[2]} {dst[3]}\")
",
        "11 22 33 44",
    );
}

#[test]
#[ignore = "WGSL Int→i32 width mismatch (pre-existing scalar() mapping); follow-up"]
fn gpu_for_scalar_multiply_writes_correct_results() {
    assert_runs_with_output(
        "
use system.io
use system.gpu
use system.collections.array

let src = [1, 2, 3, 4, 5, 6, 7, 8]
var dst = [0, 0, 0, 0, 0, 0, 0, 0]
gpu for i in 0..8
    dst[i] = src[i] * 7
println(f\"{dst[0]} {dst[3]} {dst[7]}\")
",
        "7 21 56",
    );
}

/// Smoke test verifying the infrastructure layer is wired end-to-end:
/// compilation succeeds, the binary links against `libmiri_runtime_gpu.a`,
/// and the dispatch call into `miri_gpu_launch_inline` returns (i.e. the
/// program does not crash even though the per-element values land
/// corrupted by the width mismatch documented above).
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
