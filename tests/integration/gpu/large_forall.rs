// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// A 1-D `forall` is not limited to one grid axis. A device dispatches at most
// 65535 workgroups per axis, so a 1-D loop longer than 65535 workgroups of
// threads spills its workgroups into the second grid axis; each thread's index
// is its flattened workgroup times the workgroup size plus its local id, and
// the bounds guard masks the tail the rounded-up grid overshoots.

use super::device::require_gpu_int64;
use super::utils::*;

/// Elements past the single-axis ceiling of 65535 workgroups of 256 threads
/// (16,776,960). The samples straddle that ceiling and include the last index.
const LENGTH: &str = "17000000";
const SAMPLES: &str = "0 16776959 16776960 16999999";

/// The loop over `data`; `binding` declares whatever `bound` names.
fn source(binding: &str, bound: &str) -> String {
    format!(
        "
use system.io
use system.collections.array

fn main()
    {binding}
    gpu var data = Array<i32, {LENGTH}>()
    forall i in 0..{bound}
        data[i] = (i % 1000000) as i32 + 1
    let host = data
    println(f\"{{host[0]}} {{host[16776959]}} {{host[16776960]}} {{host[16999999]}}\")
"
    )
}

/// Expected samples: element `i` holds `i % 1000000 + 1`.
fn expected() -> String {
    SAMPLES
        .split(' ')
        .map(|i| (i.parse::<i64>().unwrap() % 1_000_000 + 1).to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A literal bound whose grid exceeds one axis launches and writes every
/// element, including those past the old single-axis ceiling.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn literal_bound_forall_past_one_grid_axis_writes_every_element() {
    require_gpu_int64();
    assert_runs_with_output(&source("", LENGTH), &expected());
}

/// A runtime bound computes the same spilled grid on the host.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn runtime_bound_forall_past_one_grid_axis_writes_every_element() {
    require_gpu_int64();
    assert_runs_with_output(&source(&format!("let n = {LENGTH}"), "n"), &expected());
}

/// A literal 1-D loop longer than a 32-bit device index can number is refused
/// at compile time instead of wrapping thread indices negative.
#[test]
fn literal_bound_forall_past_the_device_index_range_is_refused() {
    assert_build_error(
        "
use system.collections.array

fn main()
    gpu var data = Array<i32, 4>()
    forall i in 0..2147483647
        if i < 4
            data[i] = 1
",
        "more threads than a 32-bit device index can number",
    );
}
