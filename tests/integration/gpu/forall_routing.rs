// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Routing of bare `forall` to CPU or GPU based on capture residency.
//!
//! A bare (non-`gpu`) `forall` statement routes to GPU if any captured
//! variable is gpu-resident, otherwise to CPU sequential backend.

use super::device::gpu_adapter_available;
use super::utils::*;

#[test]
fn bare_forall_with_gpu_resident_capture_routes_to_gpu() {
    if !gpu_adapter_available() {
        eprintln!("[gpu] skipped bare_forall_with_gpu_resident_capture_routes_to_gpu: no suitable adapter");
        return;
    }
    assert_runs(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let g = [1, 2, 3]
    forall i in 0..3
        let _ = g[i]
",
    );
}

#[test]
fn bare_forall_with_only_host_captures_routes_to_cpu() {
    assert_runs_with_output(
        "
use system.io

fn main()
    let a = [10, 20, 30]
    forall i in 0..3
        print(f\"{a[i]}\")
",
        "102030",
    );
}
