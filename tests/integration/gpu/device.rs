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
//! Availability is probed per scalar capability so a value test gates on the
//! capability it actually exercises:
//!   * `gpu_float_available()` — a basic f32 add round-trip; the baseline every
//!     compute adapter supports. Value asserts (`assert_gpu_runs_with_output`)
//!     gate on this so f32 value tests do not over-gate on `SHADER_INT64`.
//!   * `gpu_int64_available()` — an i64 add round-trip; true only when the
//!     adapter also supports `SHADER_INT64`. Tests launching integer kernels
//!     gate on this so they skip on an adapter that lacks 64-bit integers.
//!
//! An adapter that supports `SHADER_INT64` always supports f32, so on real GPU
//! hardware both probes agree; they diverge only on a hypothetical f32-only
//! adapter, where the split lets f32 tests run while integer tests skip.

use super::utils::assert_runs_with_output;
use std::sync::OnceLock;

/// Cache of the f32 availability result.
static GPU_FLOAT_AVAILABLE: OnceLock<bool> = OnceLock::new();
/// Cache of the i64 (`SHADER_INT64`) availability result.
static GPU_INT64_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Determine whether a working GPU adapter with basic f32 support is available
/// on this machine.
///
/// The oracle runs an f32 add round-trip through `forall`: it compiles a kernel
/// that adds two float arrays and reads the result back. Returns true only if
/// the computation succeeds and produces the expected sums, confirming device
/// availability and the baseline floating-point capability every value test
/// relies on.
///
/// The result is cached: the first call runs the probe, subsequent calls return
/// the cached answer. See [`run_availability_probe`] for the missing-adapter vs
/// harness-break contract.
pub fn gpu_float_available() -> bool {
    *GPU_FLOAT_AVAILABLE.get_or_init(|| {
        let probe_source = "
use system.gpu
use system.collections.array

gpu let probe_fa = [1.0, 2.0, 3.0, 4.0]
gpu let probe_fb = [10.0, 20.0, 30.0, 40.0]
gpu var probe_fdst = [0.0, 0.0, 0.0, 0.0]

gpu forall i in 0..4
    probe_fdst[i] = probe_fa[i] + probe_fb[i]

let probe_fhost = probe_fdst
println(f'{probe_fhost[0]} {probe_fhost[1]} {probe_fhost[2]} {probe_fhost[3]}')
";
        run_availability_probe(probe_source, "11.0 22.0 33.0 44.0")
    })
}

/// Determine whether a working GPU adapter that also supports `SHADER_INT64` is
/// available on this machine.
///
/// The oracle runs an i64 add round-trip through `forall`: it compiles a kernel
/// that adds two integer arrays and reads the result back. Returns true only if
/// the computation succeeds and produces the expected sums, confirming both
/// device availability and the 64-bit integer capability integer kernels need.
///
/// The result is cached: the first call runs the probe, subsequent calls return
/// the cached answer. See [`run_availability_probe`] for the missing-adapter vs
/// harness-break contract.
pub fn gpu_int64_available() -> bool {
    *GPU_INT64_AVAILABLE.get_or_init(|| {
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
        run_availability_probe(probe_source, "11 22 33 44")
    })
}

/// Require that a GPU adapter supporting `SHADER_INT64` is available.
///
/// Under the value suite (a `gpu_hardware` build) a missing int64-capable
/// adapter is a harness break, not a skip: the hardware job guarantees a real
/// GPU, so fail loudly instead of passing green.
///
/// Panics if `gpu_int64_available()` returns false.
pub fn require_gpu_int64() {
    if !gpu_int64_available() {
        panic!("harness break: gpu_hardware build but no SHADER_INT64-capable GPU adapter");
    }
}

/// Run a GPU availability probe and report whether the device produced the
/// expected output.
///
/// **Contract**: a missing or unusable GPU adapter (e.g. on a GPU-less CI
/// runner) returns `false` so callers skip — it is an expected environment
/// condition, not a harness break. Any *other* probe failure (compile, link,
/// or codegen error) still panics, since that indicates a real harness break.
/// `true` is returned only when the probe runs and produces `expected`.
fn run_availability_probe(source: &str, expected: &str) -> bool {
    let result = crate::utils::miri_run(source);
    if result.success {
        return result.output().contains(expected);
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
}

/// Assert that a GPU program compiles, runs, and produces expected output
/// if a GPU adapter is available; otherwise just assert that it compiles
/// and runs without crashing.
///
/// This abstraction keeps test code uniform: tests using this function do not
/// need to branch on GPU availability. It gates on [`gpu_float_available`] —
/// the baseline f32 capability — so f32 value tests do not over-gate on
/// `SHADER_INT64`. If an adapter is present, the full output is checked against
/// `expected`. If not, the test is skipped — a GPU program cannot run without
/// an adapter (the launch hard-errors), so there is nothing to assert; WGSL
/// validity is covered separately by the adapter-free `assert_gpu_wgsl_valid`
/// tests.
pub fn assert_gpu_runs_with_output(source: &str, expected: &str) {
    match decide_value_check(gpu_float_available(), cfg!(feature = "gpu_hardware")) {
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

/// An adapter that supports `SHADER_INT64` always supports f32, so the int64
/// probe can never be true while the float probe is false. This invariant holds
/// on every machine: with no adapter both are false (vacuously true); with an
/// int64-capable adapter the float baseline must also hold.
#[test]
fn int64_availability_implies_float_availability() {
    assert!(!gpu_int64_available() || gpu_float_available());
}
