// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for `forall` loops whose range *start* is a runtime Int expression
// (`a..n` with runtime `a`), not just a compile-time literal. The start is
// carried into the kernel via a second uniform scalar and the per-thread index
// is computed as `i = thread + start_uniform`.

use super::device::{assert_gpu_runs_with_output, require_gpu_int64};
use super::helpers::assert_gpu_wgsl_valid;

/// A runtime range start type-checks and emits valid WGSL with a uniform-bound
/// binding for the start scalar.
#[test]
fn wgsl_valid_with_runtime_start() {
    let source = "
use system.gpu
use system.collections.array

fn main()
    let a = 1
    let n = 4
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in a..n
        dst[i] = i * 10
";
    assert_gpu_wgsl_valid(source);
}

/// End-to-end: a runtime start writes only the `[start, end)` sub-range,
/// leaving elements below `start` untouched at their sentinel.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn runtime_start_writes_sub_range() {
    let source = "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = [7, 7, 7, 7]
    let a = 1
    let n = 4
    gpu forall i in a..n
        dst[i] = i * 10
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    // i = 1,2,3 written to 10,20,30; index 0 stays sentinel 7.
    assert_gpu_runs_with_output(source, "7 10 20 30");
}

/// Both start and end are runtime expressions.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn runtime_start_and_runtime_end() {
    let source = "
use system.gpu
use system.collections.array

fn main()
    gpu let src = [10, 20, 30, 40, 50]
    gpu var dst = [0, 0, 0, 0, 0]
    let a = 2
    let n = 5
    gpu forall i in a..n
        dst[i] = src[i]
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]}')
";
    // Only indices 2,3,4 are written; 0,1 stay 0.
    assert_gpu_runs_with_output(source, "0 0 30 40 50");
}

/// A runtime start equal to the end is an empty range: no writes.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn runtime_start_equal_to_end_is_noop() {
    let source = "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = [5, 5, 5, 5]
    let a = 4
    let n = 4
    gpu forall i in a..n
        dst[i] = 99
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "5 5 5 5");
}

/// A runtime start greater than the end is an empty range: no writes.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn runtime_start_greater_than_end_is_noop() {
    let source = "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = [5, 5, 5, 5]
    let a = 3
    let n = 1
    gpu forall i in a..n
        dst[i] = 99
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "5 5 5 5");
}

/// Inclusive runtime range with a runtime start includes the end element.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn runtime_start_inclusive_range_includes_end() {
    let source = "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = [0, 0, 0, 0, 0]
    let a = 1
    let n = 3
    gpu forall i in a..=n
        dst[i] = i
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]}')
";
    // i = 1,2,3 (inclusive); indices 0,4 stay 0.
    assert_gpu_runs_with_output(source, "0 1 2 3 0");
}

/// The start uniform is control data, not a capture upload: two launches over
/// one `gpu var` binding pay exactly one upload.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn runtime_start_uniform_is_not_counted_as_upload() {
    require_gpu_int64();
    use super::utils::assert_runs_with_output;
    assert_runs_with_output(
        "
use system.gpu

fn main()
    gpu_reset_telemetry()
    gpu var data = [0, 0, 0, 0, 0, 0, 0, 0]
    let a = 2
    let n = 8

    gpu forall i in a..n
        data[i] = i * i

    gpu forall i in a..n
        data[i] = data[i] * 2

    let host = data
    println(f'{host[7]} {gpu_uploads()} {gpu_launches()} {gpu_readbacks()} {gpu_fences()}')
",
        "98 1 2 1 1",
    );
}
