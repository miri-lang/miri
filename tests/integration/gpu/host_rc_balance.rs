// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reference-counting balance of the *host* side of a GPU program.
//!
//! Every other GPU test in this directory needs an adapter, so it is
//! `#[ignore]`d on a machine without one — which left the host code these
//! programs lower through outside the reach of the always-on RC verifier. A
//! build needs no adapter, and the verifier runs during it, so these tests hold
//! the host side of each launch shape to the same standard as ordinary code on
//! every machine.

use crate::integration::utils::assert_builds;

#[test]
fn reduce_result_buffer_is_released_once() {
    assert_builds(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4, 5, 6, 7, 8]
    let sum = a.reduce(0, fn(acc i32, x i32) i32: acc + x)
    println(f'{sum}')
",
    );
}

#[test]
fn gpu_resident_reduce_result_buffer_is_released_once() {
    assert_builds(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let sum = a.reduce(0, fn(acc i32, x i32) i32: acc + x)
    let host_sum = sum
    println(f'{host_sum}')
",
    );
}

#[test]
fn frame_launch_dimensions_are_released() {
    assert_builds(
        r#"
use system.io
use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu frame i in 0..4:
        b[i] = a[i] + 1
    println("ok")
"#,
    );
}

#[test]
fn frame_launch_dimensions_from_a_runtime_bound_are_released() {
    assert_builds(
        r#"
use system.io
use system.gpu

fn main()
    var n = 4
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu frame i in 0..n:
        b[i] = a[i] + 1
    println("ok")
"#,
    );
}

#[test]
fn explicit_launch_dimensions_are_released() {
    assert_builds(
        "
use system.gpu
use system.collections.array

gpu fn probe_warp_size(dst out Array<int, 1>)
    let size = kernel.warp.size
    dst[0] = size

fn main()
    gpu var dst = Array<int,1>()
    probe_warp_size(dst).launch(Dim3(1, 1, 1), Dim3(32, 1, 1))
    let result = dst
    println(f'{result[0]}')
",
    );
}

/// A grid the caller named is the caller's to release; the launch must not take
/// it over. Only the block dimension has to be a literal, so this is the one
/// dimension a program can hand over by name.
#[test]
fn a_named_grid_dimension_keeps_its_own_release() {
    assert_builds(
        "
use system.gpu
use system.collections.array

gpu fn probe_warp_size(dst out Array<int, 1>)
    let size = kernel.warp.size
    dst[0] = size

fn main()
    gpu var dst = Array<int,1>()
    let grid = Dim3(1, 1, 1)
    probe_warp_size(dst).launch(grid, Dim3(32, 1, 1))
    let result = dst
    println(f'{result[0]}')
",
    );
}

/// A kernel's own locals are not the host's to account for. `Vec3<f32>` is a
/// heap value on the host and inline storage on the device, so reading a kernel
/// body through the host's ownership rules reports a leak per vector it builds.
#[test]
fn kernel_local_vectors_are_not_host_owned() {
    assert_builds(
        "
use system.gpu
use system.gpu.vector
use system.math
use system.collections.array

fn main()
    gpu let ax = [1.0]
    gpu let ay = [0.0]
    gpu let az = [0.0]
    gpu let bx = [2.0]
    gpu let by = [3.0]
    gpu let bz = [4.0]
    gpu var result = [0.0]
    gpu forall i in 0..1
        let a = Vec3<f32>(ax[i], ay[i], az[i])
        let b = Vec3<f32>(bx[i], by[i], bz[i])
        result[i] = dot(a, b)
    let host = result
    println(f'{host[0]}')
",
    );
}

#[test]
fn tiled_matmul_launch_is_balanced() {
    assert_builds(
        "
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
",
    );
}
