// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU device availability detection and value-correctness assertions.
//!
//! This module provides wgpu-free helpers for GPU test infrastructure. The
//! availability gate is abstracted into the assert function so test code
//! is uniform regardless of GPU availability.
//!
//! `gpu_adapter_available()` returns true iff the machine has a GPU device
//! that supports SHADER_INT64. It probes by compiling and running a simple
//! int round-trip test through `forall` and checking the output.

use super::utils::{assert_runs, assert_runs_with_output};
use std::sync::OnceLock;

/// Cache of GPU availability result.
static GPU_ADAPTER_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Determine whether a working GPU adapter with SHADER_INT64 support is
/// available on this machine.
///
/// The oracle works by running an int round-trip probe through `forall`:
/// it compiles a simple kernel that adds two arrays and reads back the result.
/// Returns true only if the computation succeeds and produces the expected
/// output, confirming both device availability and the required 64-bit
/// integer capability.
///
/// **Contract**: A probe that fails to compile, link, or run is a hard error
/// (panics), indicating a broken test harness, not a missing GPU adapter.
/// `false` is returned only when the probe runs successfully but detects no
/// usable GPU (the runtime no-ops and prints "0 0 0 0").
///
/// The result is cached: the first call runs the probe, subsequent calls
/// return the cached answer.
pub fn gpu_adapter_available() -> bool {
    *GPU_ADAPTER_AVAILABLE.get_or_init(|| {
        let probe_source = "
use system.gpu
use system.io
use system.collections.array

gpu let probe_a = [1, 2, 3, 4]
gpu let probe_b = [10, 20, 30, 40]
gpu var probe_dst = [0, 0, 0, 0]

gpu forall i in 0..4
    probe_dst[i] = probe_a[i] + probe_b[i]

let probe_host = probe_dst
println(f'{probe_host[0]} {probe_host[1]} {probe_host[2]} {probe_host[3]}')
";
        let result = crate::utils::miri_run(probe_source);
        if !result.success {
            panic!(
                "GPU availability probe failed to compile, link, or run. \
                This indicates a broken test harness, not a missing GPU adapter. \
                Output: {}",
                result.output()
            );
        }
        result.output().contains("11 22 33 44")
    })
}

/// Assert that a GPU program compiles, runs, and produces expected output
/// if a GPU adapter is available; otherwise just assert that it compiles
/// and runs without crashing.
///
/// This abstraction keeps test code uniform: tests using this function do
/// not need to branch on GPU availability. If `gpu_adapter_available()`
/// is true, the full output is checked against `expected`. If false, only
/// compilation and execution success is verified.
pub fn assert_gpu_runs_with_output(source: &str, expected: &str) {
    if gpu_adapter_available() {
        assert_runs_with_output(source, expected);
    } else {
        assert_runs(source);
    }
}
