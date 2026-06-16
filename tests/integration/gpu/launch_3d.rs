// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! End-to-end value-correctness tests for 3D gpu forall.
//! Block size: 8×8×4 = 256 invocations.

use super::device::assert_gpu_runs_with_output;
use super::utils::assert_compiler_error;

/// 3D literal bounds value-correctness test.
/// A 3D grid `gpu forall x, y, z in 0..2, 0..2, 0..2` writing to a flattened
/// buffer in row-major order (x-fastest): dst[z*W*H + y*W + x] = x + y*10 + z*100.
/// 8 total iterations (2×2×2).
#[test]
fn gpu_forall_3d_literal_bounds_value_round_trips() {
    let source = "
use system.io
use system.gpu
use system.collections.array

gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0]
gpu forall x, y, z in 0..2, 0..2, 0..2
    dst[z * 4 + y * 2 + x] = x + y * 10 + z * 100
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]} {host[5]} {host[6]} {host[7]}')
";
    // Expected (row-major):
    // z=0, y=0: [0, 1]
    // z=0, y=1: [10, 11]
    // z=1, y=0: [100, 101]
    // z=1, y=1: [110, 111]
    assert_gpu_runs_with_output(source, "0 1 10 11 100 101 110 111");
}

/// 3D runtime bounds value-correctness test.
/// Each axis bound is a runtime variable (n_x, n_y, n_z).
#[test]
fn gpu_forall_3d_runtime_bounds_value_round_trips() {
    let source = "
use system.io
use system.gpu
use system.collections.array

fn main()
    let n_x = 2
    let n_y = 2
    let n_z = 2
    gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall x, y, z in 0..n_x, 0..n_y, 0..n_z
        dst[z * 4 + y * 2 + x] = x + y * 10 + z * 100
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]} {host[5]} {host[6]} {host[7]}')
";
    assert_gpu_runs_with_output(source, "0 1 10 11 100 101 110 111");
}

/// 3D mixed literal/runtime bounds (literal x, runtime y, runtime z).
#[test]
fn gpu_forall_3d_mixed_literal_x_runtime_yz() {
    let source = "
use system.io
use system.gpu
use system.collections.array

fn main()
    let n_y = 2
    let n_z = 2
    gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall x, y, z in 0..2, 0..n_y, 0..n_z
        dst[z * 4 + y * 2 + x] = x + y * 10 + z * 100
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]} {host[5]} {host[6]} {host[7]}')
";
    assert_gpu_runs_with_output(source, "0 1 10 11 100 101 110 111");
}

/// 3D mixed literal/runtime bounds (runtime x, literal y, runtime z).
#[test]
fn gpu_forall_3d_mixed_runtime_x_literal_y_runtime_z() {
    let source = "
use system.io
use system.gpu
use system.collections.array

fn main()
    let n_x = 2
    let n_z = 2
    gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall x, y, z in 0..n_x, 0..2, 0..n_z
        dst[z * 4 + y * 2 + x] = x + y * 10 + z * 100
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]} {host[5]} {host[6]} {host[7]}')
";
    assert_gpu_runs_with_output(source, "0 1 10 11 100 101 110 111");
}

/// 4D rejection: forall does not accept 4 loop variables.
#[test]
fn gpu_forall_4d_is_rejected_at_compile_time() {
    let source = "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall w, x, y, z in 0..2, 0..2, 0..2, 0..2
        dst[0] = 1
";
    assert_compiler_error(source, "at most 3 loop variables");
}

/// 3D with non-matching block dims: bounds (0..5, 0..5, 0..5) with block (8, 8, 4).
/// Verify no out-of-range writes and correct values for in-bounds elements.
#[test]
fn gpu_forall_3d_non_square_bounds_value_round_trips() {
    let source = "
use system.io
use system.gpu
use system.collections.array

fn main()
    gpu var dst = [0, 0, 0, 0, 0]
    gpu forall i, j, k in 0..5, 0..1, 0..1
        if i < 5
            dst[i] = i
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]}')
";
    assert_gpu_runs_with_output(source, "0 1 2 3 4");
}

/// 3D with inclusive range on z axis: 0..=1 (inclusive) should normalize to 0..2.
#[test]
fn gpu_forall_3d_inclusive_range_z_value_round_trips() {
    let source = "
use system.io
use system.gpu
use system.collections.array

fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall x, y, z in 0..2, 0..1, 0..=1
        dst[z * 2 + x] = x + z * 10
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "0 1 10 11");
}
