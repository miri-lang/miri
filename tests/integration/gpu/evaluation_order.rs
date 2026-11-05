// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Operands keep the value they were evaluated at inside a `forall` body, on
//! the GPU and on the CPU alike: `n + (n = 7)` with `n` holding 2 is 9. The
//! kernel is lowered in a body of its own, cut out of the function the
//! `forall` stands in, and reads the left side into a temporary there.

use super::device::require_gpu_int64;
use super::helpers::assert_gpu_wgsl_valid;
use super::utils::*;

const GPU_KERNEL: &str = "
use system.io
use system.gpu
use system.collections.array

fn main()
    gpu var result = [0, 0, 0, 0]
    gpu forall i in 0..4
        var n = i
        result[i] = n + (n = 7)
    let host = result
    println(f\"<{host.element_at(2)}>\")
";

#[test]
fn a_kernels_left_side_a_later_assignment_writes_compiles_to_valid_wgsl() {
    assert_gpu_wgsl_valid(GPU_KERNEL);
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn a_kernels_left_side_keeps_its_value_when_a_later_assignment_writes_it() {
    require_gpu_int64();
    assert_runs_with_output(GPU_KERNEL, "<9>");
}

#[test]
fn a_cpu_foralls_left_side_keeps_its_value_when_a_later_assignment_writes_it() {
    assert_runs_with_output(
        "
use system.io
use system.collections.array

fn main()
    var result = [0, 0, 0, 0]
    forall i in 0..4
        var n = i
        result[i] = n + (n = 7)
    println(f\"<{result.element_at(2)}>\")
",
        "<9>",
    );
}
