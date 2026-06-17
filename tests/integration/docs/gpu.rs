// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU documentation snippets: compile-verified examples for the Getting Started guide.
//!
//! Each test in this module corresponds to a documentation example that will be copied
//! verbatim into the public guide. All snippets are verified to compile and run correctly.

use super::super::gpu::device::assert_gpu_runs_with_output;
use super::super::utils::assert_compiler_error;

/// Vector addition: the simplest GPU kernel.
///
/// Demonstrates:
///   * `gpu let` to declare read-only device buffers
///   * `gpu var` to declare a mutable device buffer
///   * `forall` to launch a kernel over the buffer size
///   * Cross-residency readback (`let host = gpu_var`) to copy result back
///   * Printing a few elements of the result
///
/// Expected output: `6 8 10 12` (element-wise sums of [1,2,3,4] + [5,6,7,8])
#[test]
fn doc_vector_add() {
    assert_gpu_runs_with_output(
        "
use system.gpu
use system.io
use system.collections.array

const N = 4

gpu let a = [1.0, 2.0, 3.0, 4.0]
gpu let b = [5.0, 6.0, 7.0, 8.0]
gpu var dst = [0.0, 0.0, 0.0, 0.0]

gpu forall i in 0..N
    dst[i] = a[i] + b[i]

let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
",
        "6.0 8.0 10.0 12.0",
    );
}

/// Buffer reuse across multiple kernels.
///
/// Demonstrates:
///   * Multiple `forall` blocks operating on the same `gpu var` without readback
///   * Persistent device buffer optimization: the buffer is uploaded once
///   * GPU cost telemetry: `gpu_reset_telemetry()`, `gpu_uploads()`, `gpu_launches()`, etc.
///   * Expected cost-class behavior:
///     - 1 upload (when data is first declared)
///     - 2 launches (one per gpu forall block)
///     - 1 readback (when host = data)
///     - 1 fence (synchronization point for the readback)
///
/// Expected output: `23 1 2 1 1`
///   * host[7] = 7 + 8 = 15, then 15 + 8 = 23
///   * 1 upload, 2 launches, 1 readback, 1 fence
#[test]
fn doc_buffer_reuse() {
    assert_gpu_runs_with_output(
        "
use system.gpu
use system.io

const N = 8

gpu_reset_telemetry()
gpu var data = [0, 0, 0, 0, 0, 0, 0, 0]

gpu forall i in 0..N
    data[i] = i + 8

gpu forall i in 0..N
    data[i] = data[i] + 8

let host = data
println(f'{host[7]} {gpu_uploads()} {gpu_launches()} {gpu_readbacks()} {gpu_fences()}')
",
        "23 1 2 1 1",
    );
}

/// Matrix multiplication: an embarrassingly parallel kernel.
///
/// Demonstrates:
///   * Index arithmetic inside the kernel body
///   * Loop within a kernel: `var sum = 0.0; var k = 0; while k < 2 ...`
///   * Capturing immutable device buffers and writing to a device buffer
///   * 2×2 matrix multiplication: C = A × B
///
/// Layout (row-major): A = [1,2,3,4], B = [5,6,7,8]
/// Expected: C[0,0] = 1*5 + 2*7 = 19, C[0,1] = 1*6 + 2*8 = 22, etc.
/// Output: `19 22 43 50`
#[test]
fn doc_matmul() {
    assert_gpu_runs_with_output(
        "
use system.gpu
use system.io
use system.collections.array

gpu let a = [1.0, 2.0, 3.0, 4.0]
gpu let b = [5.0, 6.0, 7.0, 8.0]
gpu var c = Array<f32, 4>()

gpu forall idx in 0..4
    let row = idx / 2
    let col = idx - row * 2
    var sum = 0.0
    var k = 0
    while k < 2
        sum = sum + a[row * 2 + k] * b[k * 2 + col]
        k = k + 1
    c[idx] = sum

let host = c
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
",
        "19.0 22.0 43.0 50.0",
    );
}

/// SAXPY: single-precision a*x plus y.
///
/// Demonstrates:
///   * Scalar literals (e.g., 2.0) inlined directly in the kernel body
///   * Note: host variables CANNOT be captured into `forall` blocks, so scalars must
///     be written as literals or computed from array indices
///   * Fused multiply-add pattern commonly used in linear algebra
///
/// Computation: dst[i] = 2.0 * x[i] + y[i]
/// Expected: [2*1+5, 2*2+6, 2*3+7, 2*4+8] = [7, 10, 13, 16]
#[test]
fn doc_saxpy() {
    assert_gpu_runs_with_output(
        "
use system.gpu
use system.io

const N = 4

gpu let x = [1.0, 2.0, 3.0, 4.0]
gpu let y = [5.0, 6.0, 7.0, 8.0]
gpu var dst = [0.0, 0.0, 0.0, 0.0]

gpu forall i in 0..N
    dst[i] = 2.0 * x[i] + y[i]

let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
",
        "7.0 10.0 13.0 16.0",
    );
}

/// Correct pattern: readback then host loop.
///
/// Demonstrates:
///   * The proper way to read GPU results on the host
///   * After a `forall` block, a gpu-resident binding can only be accessed as a whole
///     via cross-residency readback: `let h = gpu_var`
///   * Once on the host, the result can be looped over freely
///   * This pattern incurs exactly one GPU-to-host readback transfer
///
/// The kernel computes data[i] = i*i, then we readback and print the first 4 elements.
#[test]
fn doc_readback_then_host_loop() {
    assert_gpu_runs_with_output(
        "
use system.gpu
use system.io

gpu var arr = [0, 0, 0, 0, 0, 0, 0, 0]

gpu forall i in 0..8
    arr[i] = i * i

let h = arr

for j in 0..4
    println(f'{h[j]}')
",
        "0
1
4
9",
    );
}

/// Forbidden pattern: direct element read from gpu-resident binding on host.
///
/// Demonstrates:
///   * An element read from a gpu-resident binding (e.g., `let v = arr[i]`) is rejected
///     at compile time
///   * The compiler error suggests the correct fix: bulk readback with `let h = arr`
///   * This prevents costly per-element transfers that would bypass the persistent
///     device buffer cost model
///
/// Expected error: "a per-element read would require a readback"
#[test]
fn doc_forbidden_element_read() {
    assert_compiler_error(
        "
use system.gpu
use system.io

fn main()
    gpu var arr = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i in 0..8
        arr[i] = i * i
    let v = arr[0]
",
        "a per-element read would require a readback",
    );
}
