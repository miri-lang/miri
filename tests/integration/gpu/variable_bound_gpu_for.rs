// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for variable-bound `forall` loops.
// The range end can be a runtime Int expression (not just a literal).

use super::device::{assert_gpu_runs_with_output, gpu_adapter_available};
use super::helpers::assert_gpu_wgsl_valid;
use super::utils::{assert_runs_with_output, assert_runtime_crash};

/// AC3: WGSL emission produces valid WGSL with uniform binding.
#[test]
fn wgsl_valid_with_runtime_bound() {
    let source = "
use system.gpu
use system.collections.array

fn main()
    let n = 4
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..n
        dst[i] = a[i]
";
    assert_gpu_wgsl_valid(source);
}

/// AC4: End-to-end dispatch with runtime Int end.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn runtime_int_variable_bound_dispatches_correctly() {
    let source = "
use system.gpu
use system.collections.array

gpu let a = [10, 20, 30, 40]
gpu let b = [1, 2, 3, 4]
gpu var dst = [0, 0, 0, 0]

let n = 4
gpu forall i in 0..n
    dst[i] = a[i] + b[i]

let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "11 22 33 44");
}

/// AC4: Runtime end via method call on gpu-resident buffer.
/// e.g., `gpu forall i in 0..g.length()` where g is `gpu let`.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn runtime_bound_via_gpu_buffer_length() {
    let source = "
use system.gpu
use system.collections.array

gpu let src = [5, 6, 7, 8, 9]
gpu var dst = [0, 0, 0, 0, 0]

gpu forall i in 0..src.length()
    dst[i] = src[i] * 2

let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]}')
";
    assert_gpu_runs_with_output(source, "10 12 14 16 18");
}

/// AC5: Empty range (n=0) is a clean no-op.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn empty_runtime_range_is_noop() {
    let source = "
use system.gpu
use system.collections.array

gpu var dst = [999, 999, 999, 999]
let n = 0
gpu forall i in 0..n
    dst[i] = i + 100
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "999 999 999 999");
}

/// AC5: Negative runtime range should also be a clean no-op.
/// The MIR lowering should clamp the grid to 0 threads.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn negative_runtime_range_is_noop() {
    let source = "
use system.gpu
use system.collections.array

gpu var dst = [888, 888, 888, 888]
let n = -5
gpu forall i in 0..n
    dst[i] = i + 100
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "888 888 888 888");
}

/// Inclusive runtime range iterates over correct element count.
/// `0..=n` where n=4 should iterate i=0,1,2,3,4 (5 total), not 4.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn inclusive_runtime_range_includes_end() {
    let source = "
use system.gpu
use system.collections.array

gpu var dst = [999, 999, 999, 999, 999]
let n = 4
gpu forall i in 0..=n
    dst[i] = i
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]}')
";
    assert_gpu_runs_with_output(source, "0 1 2 3 4");
}

/// Over-dispatch guard: with a runtime bound `n` smaller than the captured
/// buffer, the grid rounds up to a full 256-thread block, but threads `i >= n`
/// must not write. `dst[2]` / `dst[3]` stay at their initial sentinel.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn runtime_bound_threads_beyond_n_do_not_write() {
    let source = "
use system.gpu
use system.collections.array

gpu let src = [10, 20]
gpu var dst = [999, 999, 999, 999]
let n = 2
gpu forall i in 0..n
    dst[i] = src[i]
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "10 20 999 999");
}

/// The runtime bound is carried in a uniform buffer, which is control data and
/// must NOT be counted as a capture upload. This test mirrors the persistent-buffer
/// telemetry shape with a runtime bound: one `gpu var` binding pays exactly one
/// upload regardless of the two launches. If the uniform were counted, uploads
/// would be 3 (one per launch). Asserting `1` proves the exclusion.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn runtime_bound_uniform_is_not_counted_as_upload() {
    if !gpu_adapter_available() {
        eprintln!(
            "[gpu] skipped runtime_bound_uniform_is_not_counted_as_upload: no suitable adapter"
        );
        return;
    }
    assert_runs_with_output(
        "
use system.gpu

fn main()
    gpu_reset_telemetry()
    gpu var data = [0, 0, 0, 0, 0, 0, 0, 0]
    let n = 8

    gpu forall i in 0..n
        data[i] = i * i

    gpu forall i in 0..n
        data[i] = data[i] * 2

    let host = data
    println(f'{host[7]} {gpu_uploads()} {gpu_launches()} {gpu_readbacks()} {gpu_fences()}')
",
        "98 1 2 1 1",
    );
}

/// F30 A3: Bound value exceeding u32::MAX triggers GridTooLarge error and aborts.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn bound_exceeds_u32_max_errors() {
    if !gpu_adapter_available() {
        eprintln!("[gpu] skipped bound_exceeds_u32_max_errors: no suitable adapter");
        return;
    }
    let source = "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [0, 0, 0, 0]
    let n = 5000000000
    gpu forall i in 0..n
        data[i] = i
";
    assert_runtime_crash(source);
}

/// F30 A2: Negative bound should write 0 and result in empty dispatch (no error).
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn negative_bound_is_noop_no_error() {
    if !gpu_adapter_available() {
        eprintln!("[gpu] skipped negative_bound_is_noop_no_error: no suitable adapter");
        return;
    }
    let source = "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [999, 999, 999, 999]
    let n = -10
    gpu forall i in 0..n
        data[i] = i
    let host = data
    println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "999 999 999 999");
}

/// F30 A1: Bound value of 2³¹ (2147483648) exceeds device max_compute_workgroups_per_dimension
/// (~8M on most devices) and must trigger GridTooLarge error. This bound fits in u32, but the
/// resulting grid (~8.4M) exceeds the device limit, not the u32 range.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn bound_2_to_31_exceeds_device_grid_limit_errors() {
    if !gpu_adapter_available() {
        eprintln!(
            "[gpu] skipped bound_2_to_31_exceeds_device_grid_limit_errors: no suitable adapter"
        );
        return;
    }
    let source = "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [0, 0, 0, 0]
    let n = 2147483648
    gpu forall i in 0..n
        data[i] = i
";
    assert_runtime_crash(source);
}

/// When the runtime end is far below the literal start, the range is empty and
/// must dispatch a 0-thread kernel (no-op). A naive clamp on `(end - start) > 0`
/// is vulnerable to i64 subtraction underflow wrapping to a positive value; the
/// clamp instead compares the original operands (`end > start`), which stays
/// correct at the integer boundary. The runtime end here underflows the
/// subtraction yet must still be treated as an empty range.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn wraparound_runtime_range_is_noop() {
    if !gpu_adapter_available() {
        eprintln!("[gpu] skipped wraparound_runtime_range_is_noop: no suitable adapter");
        return;
    }
    let source = "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = [777, 777, 777, 777]
    let very_negative = -(9223372036854775807)
    gpu forall i in 1000..very_negative
        dst[i] = i + 100
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "777 777 777 777");
}
