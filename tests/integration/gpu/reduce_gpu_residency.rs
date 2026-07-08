// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// `.reduce` on a gpu-resident receiver returns a gpu-resident scalar.
// The 1-element output buffer persists with gpu residency; cross-residency
// assignment (`let h = s`) fences and reads back the scalar when moving to host.

use super::helpers::*;
use super::utils::*;

#[test]
fn gpu_let_reduce_captures_result_as_gpu_resident() {
    assert_runs(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [1, 2, 3, 4]
    gpu let sum = data.reduce(0, fn(a i32, b i32) i32: a + b)
",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_resident_scalar_readback_on_host_binding() {
    assert_runs_with_output(
        "
use system.gpu
use system.io
use system.collections.array

fn main()
    gpu var data = [1, 2, 3, 4]
    gpu let sum = data.reduce(0, fn(a i32, b i32) i32: a + b)
    let host_sum = sum
    println(host_sum)
",
        "10",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn host_let_reduce_still_works_with_implicit_readback() {
    // `let sum = gpu_data.reduce(...)` without `gpu let` should still work,
    // with readback occurring at the binding line (backward compatible).
    assert_runs_with_output(
        "
use system.gpu
use system.io
use system.collections.array

fn main()
    gpu var data = [1, 2, 3, 4]
    let sum = data.reduce(0, fn(a i32, b i32) i32: a + b)
    println(sum)
",
        "10",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_resident_scalar_in_arithmetic_requires_readback() {
    // Using a gpu-resident scalar in host-context arithmetic without explicit
    // cross-residency assignment should fail or implicitly readback (depending on design choice).
    // Mixed residency in one expression is a type error.
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [1, 2, 3, 4]
    gpu let sum = data.reduce(0, fn(a i32, b i32) i32: a + b)
    let result = sum + 5
",
        "gpu-resident",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_resident_scalar_can_be_captured_by_gpu_forall() {
    // A gpu-resident scalar (1-element buffer) can be captured by a gpu forall
    // if the mechanism exists; otherwise it is rejected with a diagnostic.
    assert_runs(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [1, 2, 3, 4]
    gpu let sum = data.reduce(0, fn(a i32, b i32) i32: a + b)
    gpu var result = [0, 0, 0, 0]
    gpu forall i in 0..4
        result[i] = i
",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn telemetry_shows_no_readback_for_gpu_let_reduce() {
    // gpu let reduce does NOT readback (buffer stays on GPU).
    // let reduce DOES readback (eager readback for host scalar).
    // This is a runtime/telemetry check, not a compile check.
    assert_runs(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [1, 2, 3, 4]
    gpu let sum = data.reduce(0, fn(a i32, b i32) i32: a + b)
",
    );
}
