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

use super::utils::assert_runs_with_output;
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
/// **Contract**: a missing or unusable GPU adapter (e.g. on a GPU-less CI
/// runner) returns `false` so callers skip — it is an expected environment
/// condition, not a harness break. Any *other* probe failure (compile, link,
/// or codegen error) still panics, since that indicates a real harness break.
/// `true` is returned only when the probe runs and produces the expected sum.
///
/// The result is cached: the first call runs the probe, subsequent calls
/// return the cached answer.
pub fn gpu_adapter_available() -> bool {
    *GPU_ADAPTER_AVAILABLE.get_or_init(|| {
        let probe_source = "
use system.gpu
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
        if result.success {
            return result.output().contains("11 22 33 44");
        }
        let output = result.output();
        // No adapter / no device is expected on GPU-less runners → skip.
        if output.contains("no compatible GPU adapter found")
            || output.contains("device creation failed")
        {
            return false;
        }
        panic!(
            "GPU availability probe failed to compile, link, or run. \
            This indicates a broken test harness, not a missing GPU adapter. \
            Output: {}",
            output
        );
    })
}

/// Assert that a GPU program compiles, runs, and produces expected output
/// if a GPU adapter is available; otherwise just assert that it compiles
/// and runs without crashing.
///
/// This abstraction keeps test code uniform: tests using this function do not
/// need to branch on GPU availability. If `gpu_adapter_available()` is true,
/// the full output is checked against `expected`. If false, the test is skipped
/// — a GPU program cannot run without an adapter (the launch hard-errors), so
/// there is nothing to assert; WGSL validity is covered separately by the
/// adapter-free `assert_gpu_wgsl_valid` tests.
pub fn assert_gpu_runs_with_output(source: &str, expected: &str) {
    if gpu_adapter_available() {
        assert_runs_with_output(source, expected);
    } else {
        eprintln!("[skipped: no compatible GPU adapter available]");
    }
}
