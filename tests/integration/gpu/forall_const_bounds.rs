// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Value-correctness for `gpu forall` whose axis bounds are named module-level
//! `const`s (`const A = ...`, `const B = ...`; `A..B`) across 1D, 2D, and 3D.
//!
//! A named `const` bound const-folds to a compile-time integer, so both the
//! start offset (`AxisStart::Literal`) and the loop limit resolve to constants.
//! These tests pin that the folded start+end pair produces the same grid as the
//! equivalent integer-literal range, including a non-zero const start where the
//! start offset must be baked into `i = thread + start`.

use super::device::assert_gpu_runs_with_output;

/// 1D: `const A = 0; const B = 10; gpu forall x in A..B`. dst[x] = x.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_forall_1d_const_bounds_value_round_trips() {
    let source = "
use system.gpu
use system.collections.array

const A = 0
const B = 10

gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
gpu forall x in A..B
    dst[x] = x
let host = dst
println(f'{host[0]} {host[3]} {host[9]}')
";
    assert_gpu_runs_with_output(source, "0 3 9");
}

/// 1D non-zero const start: `const A = 2; const B = 10`. dst[0] and dst[1] stay
/// unwritten (x starts at 2); dst[x] = x for x in 2..10. Exercises the folded
/// start offset baked into the kernel index.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_forall_1d_nonzero_const_start_value_round_trips() {
    let source = "
use system.gpu
use system.collections.array

const A = 2
const B = 10

gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
gpu forall x in A..B
    dst[x] = x
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[5]} {host[9]}')
";
    // dst[0], dst[1] never written (loop starts at 2).
    assert_gpu_runs_with_output(source, "0 0 2 5 9");
}

/// 2D: `gpu forall x, y in A..B, A..B` over a 4×4 grid. dst[y*4 + x] = x + y*10.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_forall_2d_const_bounds_value_round_trips() {
    let source = "
use system.gpu
use system.collections.array

const A = 0
const B = 4

gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
gpu forall x, y in A..B, A..B
    dst[y * 4 + x] = x + y * 10
let host = dst
println(f'{host[0]} {host[6]} {host[15]}')
";
    // dst[0]=0 (x=0,y=0); dst[6]=x2,y1 => 2+10=12; dst[15]=x3,y3 => 3+30=33.
    assert_gpu_runs_with_output(source, "0 12 33");
}

/// 3D: `gpu forall x, y, z in A..B, A..B, A..B` over a 4×4×4 grid.
/// dst[z*16 + y*4 + x] = x + y*10 + z*100.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_forall_3d_const_bounds_value_round_trips() {
    let source = "
use system.gpu
use system.collections.array

const A = 0
const B = 4

gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
gpu forall x, y, z in A..B, A..B, A..B
    dst[z * 16 + y * 4 + x] = x + y * 10 + z * 100
let host = dst
println(f'{host[0]} {host[21]} {host[63]}')
";
    // dst[0]=0; dst[21]=x1,y1,z1 => 1+10+100=111; dst[63]=x3,y3,z3 => 3+30+300=333.
    assert_gpu_runs_with_output(source, "0 111 333");
}
