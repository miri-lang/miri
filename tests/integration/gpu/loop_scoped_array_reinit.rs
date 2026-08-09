// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Correctness of array re-initialization inside GPU loops.
//!
//! Arrays declared inside a loop must be re-zeroed at their declaration point
//! on each iteration, rather than persisting with values from the previous
//! iteration. WGSL zero-initializes `var<function>` storage once at function
//! entry, not at each loop iteration, so the emitter must emit an explicit
//! assignment when the constructor is invoked inside a loop.

use super::device::assert_gpu_runs_with_output;

/// Loop-scoped array must be re-zeroed each iteration.
/// The reproduction case from the bug report: a loop writes to a loop-scoped
/// array and reads it back. Without re-initialization, the array accumulates
/// values across iterations.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_loop_scoped_array_reinit_each_iteration() {
    let source = "
use system.io
use system.gpu
use system.collections.array

gpu var out = Array<f32, 4>()

forall i in 0..4
    var e = 0
    while e < 3
        var g = Array<f32, 4>()
        g[i] = g[i] + 1.0
        out[i] = g[i]
        e = e + 1

let host = out
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    // Each iteration of the while loop should see a fresh g[i]=0, so g[i]+1.0=1.0.
    // Without re-init, g persists and accumulates to 3.0 after 3 iterations.
    assert_gpu_runs_with_output(source, "1.0 1.0 1.0 1.0");
}

/// Array declared inside an if block within a loop must be re-initialized
/// when the if block is re-entered on a new loop iteration.
/// The if condition is true on each iteration so the block is re-entered,
/// and the array must be re-zeroed each time, not persist across iterations.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_loop_scoped_array_in_if_block_reinit() {
    let source = "
use system.io
use system.gpu
use system.collections.array

gpu var out = Array<f32, 4>()

forall i in 0..4
    var iter = 0
    while iter < 3
        if true
            var a = Array<f32, 4>()
            a[i] = a[i] + 1.0
            out[i] = a[i]
        iter = iter + 1

let host = out
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    // Array is re-initialized on each iteration of the while loop.
    // a[i] starts at 0.0, so a[i] + 1.0 = 1.0 on each iteration.
    // Without re-init, the array persists and accumulates to 3.0 after 3 iterations.
    assert_gpu_runs_with_output(source, "1.0 1.0 1.0 1.0");
}

/// Nested loops: array declared in the inner loop must be re-initialized on
/// each inner loop iteration, not persisting across the outer loop.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_nested_loop_scoped_array_reinit() {
    let source = "
use system.io
use system.gpu
use system.collections.array

gpu var out = Array<f32, 2>()

forall i in 0..2
    var outer = 0
    while outer < 2
        var inner = 0
        while inner < 1
            var x = Array<f32, 2>()
            x[i] = x[i] + 1.0
            out[i] = x[i]
            inner = inner + 1
        outer = outer + 1

let host = out
println(f'{host[0]} {host[1]}')
";
    // Inner loop runs once per outer iteration (2 outer iterations total).
    // x[i] is re-initialized on each inner loop iteration to 0, then 0 + 1.0 = 1.0.
    // Without re-init, x persists across the inner loop boundary and accumulates.
    assert_gpu_runs_with_output(source, "1.0 1.0");
}

/// Scalar control: a loop-scoped scalar must be re-initialized correctly
/// (this is the existing behavior and must not regress). Scalars are assigned
/// explicitly in the lowering, so they always get re-zeroed on each iteration.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_loop_scoped_scalar_control() {
    let source = "
use system.io
use system.gpu
use system.collections.array

gpu var out = Array<f32, 1>()

forall i in 0..1
    var e = 0
    var last = 0.0
    while e < 3
        var s = 0.0
        s = s + 1.0
        last = s
        e = e + 1
    out[0] = last

let host = out
println(f'{host[0]}')
";
    // s is re-zeroed each iteration, so s + 1.0 = 1.0 always.
    assert_gpu_runs_with_output(source, "1.0");
}
