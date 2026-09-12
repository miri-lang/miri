// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `is_gpu_available()` answers for the adapter, not for the device.
//!
//! Checking for a GPU before using one is the natural shape, so the predicate
//! has to be correct *before* the first launch creates a device. These tests
//! pin both halves of that contract: the answer is identical on either side of
//! a launch on hardware, and it is `false` when no adapter can be reached.

use super::device::assert_gpu_runs_with_output;
use crate::utils::miri_run_with_env;

/// A launch must not be what makes the predicate true. Sampled twice before
/// and once after the same program's `forall`, the answer has to be the same
/// `true` every time — the repeated pre-launch call also covers the answer
/// being remembered rather than re-derived.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn availability_is_identical_before_and_after_a_launch() {
    assert_gpu_runs_with_output(
        "
use system.gpu

let before = is_gpu_available()
let before_again = is_gpu_available()
gpu var availability_dst = [0.0, 0.0, 0.0, 0.0]

gpu forall i in 0..4
    availability_dst[i] = 7.0

let availability_host = availability_dst
let after = is_gpu_available()
println(f'{before} {before_again} {after} {availability_host[0]}')
",
        "true true true 7.0",
    );
}

/// The predicate has to work in a program that never touches the GPU: nothing
/// declares a `gpu` binding, nothing launches, and the answer is still the
/// machine's. This is the shape a program uses to decide whether to take a GPU
/// path at all.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn availability_is_true_in_a_program_that_never_launches() {
    assert_gpu_runs_with_output(
        "
use system.gpu

println(f'{is_gpu_available()}')
",
        "true",
    );
}

/// Pinning `WGPU_BACKEND=noop` selects a backend that hands out no adapter —
/// the noop backend stays disabled unless separately enabled — so this
/// reproduces a machine with no GPU on any host, including one with a working
/// Metal or Vulkan adapter.
#[test]
fn availability_is_false_without_an_adapter() {
    let result = miri_run_with_env(
        "
use system.gpu

println(f'{is_gpu_available()}')
",
        "WGPU_BACKEND",
        "noop",
    );
    assert!(
        result.success,
        "probing for a GPU must not fail the program: {}",
        result.output()
    );
    assert_eq!(
        result.stdout.trim(),
        "false",
        "no reachable adapter must read as unavailable: {}",
        result.output()
    );
}

/// The `bool`-returning predicates `system.gpu` publishes, read out of the
/// stdlib source rather than listed here, so a predicate added later is gated
/// the day it lands instead of when someone remembers to add it. Counters are
/// excluded by returning `int`: a launch is supposed to move those.
fn published_bool_predicates(stdlib_source: &str) -> Vec<&str> {
    stdlib_source
        .lines()
        .filter_map(|line| line.trim().strip_prefix("public fn "))
        .filter_map(|declaration| declaration.strip_suffix("() bool"))
        .collect()
}

/// Class gate: no `system.gpu` predicate may answer one thing before a launch
/// and another after it. A predicate that describes the machine has no business
/// changing because a kernel ran; one that changed was reporting the runtime's
/// own initialization state, which is the defect `is_gpu_available` had.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn every_published_predicate_is_launch_independent() {
    let predicates = published_bool_predicates(include_str!("../../../src/stdlib/system/gpu.mi"));
    assert!(
        predicates.contains(&"is_gpu_available"),
        "the gate reads the wrong source or the declaration form changed: found {:?}",
        predicates
    );
    for predicate in &predicates {
        let program = format!(
            "
use system.gpu

let before = {predicate}()
gpu var gate_dst = [0.0, 0.0, 0.0, 0.0]

gpu forall i in 0..4
    gate_dst[i] = 1.0

let gate_host = gate_dst
let after = {predicate}()
let unchanged = before == after
println(f'{{unchanged}} {{gate_host[0]}}')
"
        );
        assert_gpu_runs_with_output(&program, "true 1.0");
    }
    eprintln!(
        "[launch-independence gate covered {} predicates]",
        predicates.len()
    );
}
