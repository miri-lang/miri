// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// End-to-end tests for user `gpu fn` launching with buffer arguments
// and caller-chosen 2D workgroup sizing.

use super::device::assert_gpu_runs_with_output;
use super::helpers::{assert_gpu_wgsl_valid, compile_to_wgsl};
use crate::integration::utils::assert_build_error;
use crate::mir::utils::mir_lowering_gpu_fn_launch_test;

/// MIR-level test: verify that `gpu fn` call with buffer args produces
/// a GpuLaunch with correct read/write flags based on `out` parameters.
#[test]
fn gpu_fn_launch_with_buffers_lowers_to_correct_mir() {
    mir_lowering_gpu_fn_launch_test(
        "
use system.collections.array

gpu fn tiled_matmul(a Array<f32,4>, b Array<f32,4>, c out Array<f32,4>)
    let x = 1

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu let b = [5.0, 6.0, 7.0, 8.0]
    gpu var c = Array<f32,4>()
    tiled_matmul(a, b, c).launch(Dim3(1, 1, 1), Dim3(2, 2, 1))
",
        3,                       // expected num_buffers
        vec![true, true, false], // c is marked `out`, so it's writable (read_only=false)
    );
}

/// MIR lowering error: passing a host-resident array to a `.launch` call should fail.
/// This is caught during lowering (when the kernel is actually dispatched), not type-checking.
#[test]
fn gpu_fn_launch_with_host_array_rejected() {
    assert_build_error(
        "
use system.collections.array

gpu fn my_kernel(a Array<f32,4>)
    let x = 1

fn main()
    let a = [1.0, 2.0, 3.0, 4.0]
    my_kernel(a).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
",
        "host-resident",
    );
}

/// WGSL-level validation: the emitted WGSL contains the correct
/// `@compute @workgroup_size(2, 2, 1)` and shared memory declarations.
#[test]
fn gpu_fn_launch_with_workgroup_size_produces_correct_wgsl() {
    let source = "
use system.collections.array

gpu fn tiled_matmul(a Array<f32,4>, b Array<f32,4>, c out Array<f32,4>)
    shared tileA Array<f32, 4>
    shared tileB Array<f32, 4>
    let sum = 0.0

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu let b = [5.0, 6.0, 7.0, 8.0]
    gpu var c = Array<f32,4>()
    tiled_matmul(a, b, c).launch(Dim3(1, 1, 1), Dim3(2, 2, 1))
";

    let wgsl = compile_to_wgsl(source);
    assert!(
        wgsl.contains("@compute @workgroup_size(2, 2, 1)"),
        "WGSL missing workgroup_size attribute"
    );
    assert!(
        wgsl.contains("var<workgroup>"),
        "WGSL missing workgroup storage class"
    );
    assert_gpu_wgsl_valid(source);
}

/// Metal value verification (2×2 case): single tile, grid(1,1,1) block(2,2,1).
/// A=[1,2,3,4], B=[5,6,7,8] (both 2×2 row-major).
/// C[i,j] = Σ_k A[i,k] * B[k,j]:
/// C[0,0] = 1*5 + 2*7 = 19, C[0,1] = 1*6 + 2*8 = 22,
/// C[1,0] = 3*5 + 4*7 = 43, C[1,1] = 3*6 + 4*8 = 50.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn tiled_matmul_2x2_single_tile() {
    let source = "
use system.collections.array

gpu fn tiled_matmul(a Array<f32,4>, b Array<f32,4>, c out Array<f32,4>)
    shared tileA Array<f32, 4>
    shared tileB Array<f32, 4>
    let tx = kernel.thread_idx.x
    let ty = kernel.thread_idx.y

    tileA[ty*2 + tx] = a[ty*2 + tx]
    tileB[ty*2 + tx] = b[ty*2 + tx]
    kernel.barrier()

    var acc = 0.0
    let k = 0
    acc = acc + tileA[ty*2 + k] * tileB[k*2 + tx]
    acc = acc + tileA[ty*2 + (k+1)] * tileB[(k+1)*2 + tx]

    c[ty*2 + tx] = acc

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu let b = [5.0, 6.0, 7.0, 8.0]
    gpu var c = Array<f32,4>()
    tiled_matmul(a, b, c).launch(Dim3(1, 1, 1), Dim3(2, 2, 1))
    let host = c
    println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "19.0 22.0 43.0 50.0");
}

/// Metal value verification (4×4 case): multi-tile with 2×2 grid and K-loop.
/// A=[1..16] (row-major), B=all-ones.
/// C[i,j] = Σ_k A[i,k] = row sums: [10,10,10,10, 26,26,26,26, 42,42,42,42, 58,58,58,58].
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn tiled_matmul_4x4_multi_tile() {
    let source = "
use system.collections.array

gpu fn tiled_matmul(a Array<f32,16>, b Array<f32,16>, c out Array<f32,16>)
    shared tileA Array<f32, 4>
    shared tileB Array<f32, 4>
    let tx = kernel.thread_idx.x
    let ty = kernel.thread_idx.y
    let bx = kernel.block_idx.x
    let by = kernel.block_idx.y

    let row = by*2 + ty
    let col = bx*2 + tx

    var acc = 0.0
    let tile_k = 0
    tileA[ty*2 + tx] = a[row*4 + tile_k*2 + tx]
    tileB[ty*2 + tx] = b[(tile_k*2 + ty)*4 + col]
    kernel.barrier()

    let k = 0
    acc = acc + tileA[ty*2 + k] * tileB[k*2 + tx]
    acc = acc + tileA[ty*2 + (k+1)] * tileB[(k+1)*2 + tx]

    kernel.barrier()

    let tile_k2 = 1
    tileA[ty*2 + tx] = a[row*4 + tile_k2*2 + tx]
    tileB[ty*2 + tx] = b[(tile_k2*2 + ty)*4 + col]
    kernel.barrier()

    acc = acc + tileA[ty*2 + k] * tileB[k*2 + tx]
    acc = acc + tileA[ty*2 + (k+1)] * tileB[(k+1)*2 + tx]

    c[row*4 + col] = acc

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0]
    gpu let b = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]
    gpu var c = Array<f32,16>()
    tiled_matmul(a, b, c).launch(Dim3(2, 2, 1), Dim3(2, 2, 1))
    let host = c
    println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]} {host[5]} {host[6]} {host[7]} {host[8]} {host[9]} {host[10]} {host[11]} {host[12]} {host[13]} {host[14]} {host[15]}')
";
    assert_gpu_runs_with_output(
        source,
        "10.0 10.0 10.0 10.0 26.0 26.0 26.0 26.0 42.0 42.0 42.0 42.0 58.0 58.0 58.0 58.0",
    );
}

/// Zero intermediate readbacks: verify only one readback happens (the final
/// cross-residency assignment), not per-buffer intermediate readbacks.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn tiled_matmul_has_no_intermediate_readbacks() {
    let source = "
use system.collections.array
use system.gpu

gpu fn tiled_matmul(a Array<f32,4>, b Array<f32,4>, c out Array<f32,4>)
    let tx = kernel.thread_idx.x
    let ty = kernel.thread_idx.y
    c[ty*2 + tx] = a[ty*2 + tx] * b[ty*2 + tx]

fn main()
    gpu_reset_telemetry()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu let b = [2.0, 2.0, 2.0, 2.0]
    gpu var c = Array<f32,4>()
    tiled_matmul(a, b, c).launch(Dim3(1, 1, 1), Dim3(2, 2, 1))
    let host = c
    println(f'{host[0]} {host[1]} {host[2]} {host[3]} {gpu_readbacks()}')
";
    assert_gpu_runs_with_output(source, "2.0 4.0 6.0 8.0 1");
}

/// Error test: launching a gpu fn with conflicting block shapes should fail.
#[test]
fn gpu_fn_launch_conflicting_block_shapes_rejected() {
    assert_build_error(
        "
use system.collections.array

gpu fn my_kernel(a Array<f32,4>)
    let x = 1

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    my_kernel(a).launch(Dim3(1, 1, 1), Dim3(2, 2, 1))
    my_kernel(a).launch(Dim3(1, 1, 1), Dim3(4, 4, 1))
",
        "conflicting launch workgroup shapes",
    );
}

/// Error test: block size must be a compile-time literal.
#[test]
fn gpu_fn_launch_non_literal_block_rejected() {
    assert_build_error(
        "
use system.collections.array

gpu fn my_kernel(a Array<f32,4>)
    let x = 1

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    var block_size = 2
    my_kernel(a).launch(Dim3(1, 1, 1), Dim3(block_size, 2, 1))
",
        "compile-time literal",
    );
}

/// Error test: block dimensions must all be > 0.
#[test]
fn gpu_fn_launch_zero_block_dim_rejected() {
    assert_build_error(
        "
use system.collections.array

gpu fn my_kernel(a Array<f32,4>)
    let x = 1

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    my_kernel(a).launch(Dim3(1, 1, 1), Dim3(0, 1, 1))
",
        ">0",
    );
}
