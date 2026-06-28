// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU subgroup (warp) intrinsics: size, lane_id, shuffle_down.
//!
//! These tests verify:
//! - `kernel.warp.size` property read (u32, at runtime from subgroup_size builtin)
//! - `kernel.warp.lane_id` property read (u32, at runtime from subgroup_invocation_id builtin)
//! - `kernel.warp.shuffle_down(v, n)` method (compile-time literal offset n)

use super::device::assert_gpu_runs_with_output;
use super::helpers::assert_gpu_wgsl_valid;

#[test]
fn warp_size_property_emits_subgroup_size_builtin() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.collections.array

gpu fn warp_size_kernel(dst out Array<i32, 1>)
    let size = kernel.warp.size
    dst[0] = size

fn main()
    gpu var dst = Array<i32,1>()
    warp_size_kernel(dst).launch(Dim3(1, 1, 1), Dim3(32, 1, 1))
",
    );
}

#[test]
fn warp_lane_id_property_emits_subgroup_invocation_id_builtin() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.collections.array

gpu fn lane_id_kernel(dst out Array<i32, 32>)
    let lane = kernel.warp.lane_id
    dst[lane] = lane

fn main()
    gpu var dst = Array<i32,32>()
    lane_id_kernel(dst).launch(Dim3(1, 1, 1), Dim3(32, 1, 1))
",
    );
}

#[test]
fn warp_shuffle_down_method_emits_subgroup_shuffle_down() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.collections.array

gpu fn shuffle_kernel(src Array<int, 32>, dst out Array<int, 32>)
    let v = src[kernel.warp.lane_id]
    let shuffled = kernel.warp.shuffle_down(v, 1)
    dst[kernel.warp.lane_id] = shuffled

fn main()
    gpu let src = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32]
    gpu var dst = Array<int,32>()
    shuffle_kernel(src, dst).launch(Dim3(1, 1, 1), Dim3(32, 1, 1))
",
    );
}

#[test]
fn warp_shuffle_offset_128_exceeds_maximum_rejected() {
    use super::super::utils::assert_compiler_error;
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

gpu fn bad_shuffle_big_offset(dst out Array<int, 1>)
    let v = 0
    dst[0] = kernel.warp.shuffle_down(v, 200)

fn main()
    gpu var dst = Array<int,1>()
    bad_shuffle_big_offset(dst).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
",
        "exceeds the maximum subgroup size",
    );
}

#[test]
fn warp_shuffle_non_literal_offset_rejected() {
    use super::super::utils::assert_compiler_error;
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

gpu fn bad_shuffle_var_offset(n int, dst out Array<int, 1>)
    let v = 0
    dst[0] = kernel.warp.shuffle_down(v, n)

fn main()
    gpu var dst = Array<int,1>()
    bad_shuffle_var_offset(5, dst).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
",
        "must be a compile-time literal",
    );
}

/// Metal value test: warp size on this box (Apple Metal = 32 subgroup size).
/// This test documents and guards the assumption that the reduction tests rely on.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn warp_size_is_32_on_metal() {
    assert_gpu_runs_with_output(
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
        "32",
    );
}

/// Metal value test: lane_id is the invocation's position within the subgroup.
/// A 32-thread block has lanes 0..31; each lane reads its own index.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn warp_lane_id_0_to_31_in_32_thread_block() {
    assert_gpu_runs_with_output(
        "
use system.gpu
use system.collections.array

gpu fn probe_lane_ids(dst out Array<int, 32>)
    let lane = kernel.warp.lane_id
    dst[lane] = lane

fn main()
    gpu var dst = Array<int,32>()
    probe_lane_ids(dst).launch(Dim3(1, 1, 1), Dim3(32, 1, 1))
    let result = dst
    println(f'{result[0]} {result[1]} {result[2]} {result[3]} {result[4]} {result[5]} {result[6]} {result[7]} {result[8]} {result[9]} {result[10]} {result[11]} {result[12]} {result[13]} {result[14]} {result[15]} {result[16]} {result[17]} {result[18]} {result[19]} {result[20]} {result[21]} {result[22]} {result[23]} {result[24]} {result[25]} {result[26]} {result[27]} {result[28]} {result[29]} {result[30]} {result[31]}')
",
        "0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31",
    );
}

/// Metal value test: shuffle reduction (warp_reduce_sum pattern from PLAN.md).
/// Single 32-thread block (one subgroup). Each lane has value (lane_id + 1).
/// Tree-reduce with offsets 16, 8, 4, 2, 1 → lane 0 has sum of all input values.
/// Sum of 1..32 = 528.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn warp_shuffle_reduction_sum_1_to_32_equals_528() {
    assert_gpu_runs_with_output(
        "
use system.gpu
use system.collections.array

gpu fn warp_reduce_sum(input Array<int, 32>, dst out Array<int, 1>)
    let lane = kernel.warp.lane_id
    var v = input[lane]

    v = v + kernel.warp.shuffle_down(v, 16)
    v = v + kernel.warp.shuffle_down(v, 8)
    v = v + kernel.warp.shuffle_down(v, 4)
    v = v + kernel.warp.shuffle_down(v, 2)
    v = v + kernel.warp.shuffle_down(v, 1)

    if lane == 0
        dst[0] = v

fn main()
    gpu let input = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32]
    gpu var dst = Array<int,1>()
    warp_reduce_sum(input, dst).launch(Dim3(1, 1, 1), Dim3(32, 1, 1))
    let result = dst
    println(f'{result[0]}')
",
        "528",
    );
}

/// Metal value test: shuffle reduction with f32.
/// Same pattern but with floats: all-ones f32 array, sum should be 32.0.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn warp_shuffle_reduction_float_all_ones_equals_32() {
    assert_gpu_runs_with_output(
        "
use system.gpu
use system.collections.array

gpu fn warp_reduce_sum_f32(input Array<f32, 32>, dst out Array<f32, 1>)
    let lane = kernel.warp.lane_id
    var v = input[lane]

    v = v + kernel.warp.shuffle_down(v, 16)
    v = v + kernel.warp.shuffle_down(v, 8)
    v = v + kernel.warp.shuffle_down(v, 4)
    v = v + kernel.warp.shuffle_down(v, 2)
    v = v + kernel.warp.shuffle_down(v, 1)

    if lane == 0
        dst[0] = v

fn main()
    gpu let input = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]
    gpu var dst = Array<f32,1>()
    warp_reduce_sum_f32(input, dst).launch(Dim3(1, 1, 1), Dim3(32, 1, 1))
    let result = dst
    println(f'{result[0]}')
",
        "32.0",
    );
}
