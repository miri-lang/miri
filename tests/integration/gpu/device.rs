// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU device availability detection and value-correctness assertions.
//!
//! This module provides wgpu-free helpers for GPU test infrastructure. The
//! availability gate is abstracted into the assert function so test code
//! is uniform regardless of GPU availability.
//!
//! The green/smoke boundary is explicit: value tests are `#[ignore]`-gated by
//! the `gpu_hardware` feature so they run only on the real-GPU job, and
//! `assert_gpu_runs_with_output` fails loudly if that value suite is active
//! without an adapter instead of silently degrading a value check to a green
//! no-op (see `decide_value_check`).
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
    match decide_value_check(gpu_adapter_available(), cfg!(feature = "gpu_hardware")) {
        ValueCheck::Assert => assert_runs_with_output(source, expected),
        ValueCheck::Skip => {
            eprintln!("[skipped: no compatible GPU adapter available]");
        }
        ValueCheck::HarnessBreak => panic!(
            "GPU value suite (`gpu_hardware`) ran without an available adapter. \
            The hardware job guarantees a real GPU, so a missing adapter is a \
            harness/environment break — not a silent smoke-degradation to green. \
            WGSL validity is covered separately by the adapter-free tests."
        ),
    }
}

/// What a GPU value assertion should do, given adapter availability and whether
/// the adapter-gated value suite is active (`gpu_hardware` feature).
///
/// Making this a distinct decision keeps the green/smoke boundary explicit: a
/// value test that reports green under the value suite genuinely asserted device
/// output; it never silently degrades to a no-op when the adapter is absent.
#[derive(Debug, PartialEq, Eq)]
enum ValueCheck {
    /// Adapter present — assert the exact expected output on the device.
    Assert,
    /// Adapter absent and the value suite is inactive (adapter-free build) —
    /// skip; WGSL validity is covered by the adapter-free tests.
    Skip,
    /// Adapter absent while the value suite is active — a harness break, since
    /// the hardware job guarantees a real GPU. Fail loudly rather than green.
    HarnessBreak,
}

/// Decide how a GPU value assertion resolves. Adapter present always asserts;
/// adapter absent skips only when the value suite is inactive, otherwise it is
/// a harness break.
fn decide_value_check(adapter_available: bool, value_suite_active: bool) -> ValueCheck {
    match (adapter_available, value_suite_active) {
        (true, _) => ValueCheck::Assert,
        (false, true) => ValueCheck::HarnessBreak,
        (false, false) => ValueCheck::Skip,
    }
}

#[test]
fn value_check_asserts_when_adapter_available() {
    assert_eq!(decide_value_check(true, false), ValueCheck::Assert);
    assert_eq!(decide_value_check(true, true), ValueCheck::Assert);
}

#[test]
fn value_check_skips_without_adapter_when_suite_inactive() {
    assert_eq!(decide_value_check(false, false), ValueCheck::Skip);
}

#[test]
fn value_check_is_harness_break_without_adapter_under_value_suite() {
    assert_eq!(decide_value_check(false, true), ValueCheck::HarnessBreak);
}
