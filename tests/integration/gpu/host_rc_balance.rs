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
