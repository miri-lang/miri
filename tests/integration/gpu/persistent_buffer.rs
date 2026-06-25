// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Persistent device buffer per `gpu`-resident binding. A `gpu var` binding's
// device buffer is allocated and uploaded once, then reused across every kernel
// launch that captures it; only a cross-residency readback (`let h = g`) fences
// and copies back. The residency cost counters in `src/runtime/gpu/` make this
// observable from source, so these tests assert the exact upload / launch /
// readback / fence budget end-to-end through native dispatch.

use super::device::gpu_adapter_available;
use super::utils::*;

/// Persistent buffer test: the two-stage pipeline pays exactly one upload, two
/// launches, and one readback (one fence). `host[7] = (7*7)*2 = 98`.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn two_stage_pipeline_reuses_one_device_buffer() {
    if !gpu_adapter_available() {
        eprintln!("[gpu] skipped two_stage_pipeline_reuses_one_device_buffer: no suitable adapter");
        return;
    }
    assert_runs_with_output(
        "
use system.gpu
use system.io

fn main()
    gpu_reset_telemetry()
    gpu var data = [0, 0, 0, 0, 0, 0, 0, 0]

    gpu forall i in 0..8
        data[i] = i * i

    gpu forall i in 0..8
        data[i] = data[i] * 2

    let host = data
    println(f'{host[7]} {gpu_uploads()} {gpu_launches()} {gpu_readbacks()} {gpu_fences()}')
",
        "98 1 2 1 1",
    );
}

/// Adding a third `forall` block that captures the same binding adds one
/// launch and zero uploads.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn third_capture_adds_launch_not_upload() {
    if !gpu_adapter_available() {
        eprintln!("[gpu] skipped third_capture_adds_launch_not_upload: no suitable adapter");
        return;
    }
    assert_runs_with_output(
        "
use system.gpu
use system.io

fn main()
    gpu_reset_telemetry()
    gpu var data = [0, 0, 0, 0, 0, 0, 0, 0]

    gpu forall i in 0..8
        data[i] = i * i

    gpu forall i in 0..8
        data[i] = data[i] * 2

    gpu forall i in 0..8
        data[i] = data[i] + 1

    let host = data
    println(f'{host[7]} {gpu_uploads()} {gpu_launches()}')
",
        "99 1 3",
    );
}

/// A `gpu` binding declared inside a function called more than once must
/// start each runtime lifetime fresh: the second call re-uploads its host
/// bytes instead of reusing the first call's stale device buffer. The kernel
/// reads `data[i]` on the RHS, so a stale buffer would corrupt the result.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn redeclared_binding_in_repeated_call_reuploads() {
    if !gpu_adapter_available() {
        eprintln!(
            "[gpu] skipped redeclared_binding_in_repeated_call_reuploads: no suitable adapter"
        );
        return;
    }
    assert_runs_with_output(
        "
use system.gpu
use system.io

fn run() int
    gpu var data = [0, 0, 0, 0]
    gpu forall i in 0..4
        data[i] = data[i] + 5
    let host = data
    return host[3]

fn main()
    gpu_reset_telemetry()
    let a = run()
    let b = run()
    println(f'{a} {b} {gpu_uploads()} {gpu_launches()}')
",
        "5 5 2 2",
    );
}

/// A `gpu`-resident binding survives two readbacks, each fencing once and
/// producing an independent host array. The persistent buffer means a single
/// upload still covers both stages.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn two_readbacks_each_fence_and_survive() {
    if !gpu_adapter_available() {
        eprintln!("[gpu] skipped two_readbacks_each_fence_and_survive: no suitable adapter");
        return;
    }
    assert_runs_with_output(
        "
use system.gpu
use system.io

fn main()
    gpu_reset_telemetry()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
        arr[i] = i * i

    let h = arr
    let h2 = arr
    println(f'{h[3]} {h2[3]} {gpu_uploads()} {gpu_readbacks()}')
",
        "9 9 1 2",
    );
}

/// Assigning a host array to a gpu-resident binding performs a real upload.
/// The kernel reads the assigned values, proving the upload is not a silent
/// host-side copy that skips device transfer.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_var_assignment_from_host_array_uploads() {
    if !gpu_adapter_available() {
        eprintln!("[gpu] skipped gpu_var_assignment_from_host_array_uploads: no suitable adapter");
        return;
    }
    assert_runs_with_output(
        "
use system.gpu
use system.io

fn main()
    gpu_reset_telemetry()
    gpu var data = [1, 1, 1, 1]
    gpu forall i in 0..4
        data[i] = data[i] + 10

    let host1 = data
    println(f'{host1[0]} {host1[1]} {host1[2]} {host1[3]}')

    data = [2, 2, 2, 2]

    gpu forall i in 0..4
        data[i] = data[i] + 20

    let host2 = data
    println(f'{host2[0]} {host2[1]} {host2[2]} {host2[3]} {gpu_uploads()}')
",
        "11 11 11 11\n22 22 22 22 2",
    );
}

/// Simpler test: upload without a launch on host-only system.
/// Since no GPU adapter exists, this just tests the structure is correct.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_var_assignment_uploads_count() {
    if !gpu_adapter_available() {
        eprintln!("[gpu] skipped gpu_var_assignment_uploads_count: no suitable adapter");
        return;
    }
    assert_runs_with_output(
        "
use system.gpu
use system.io

fn main()
    gpu_reset_telemetry()
    gpu var data = [1, 1, 1, 1]
    gpu forall i in 0..4
        data[i] = data[i] * 2

    println(f'after first launch: {gpu_uploads()}')

    data = [3, 3, 3, 3]
    println(f'after assignment: {gpu_uploads()}')

    gpu forall i in 0..4
        data[i] = data[i] + 1

    let host = data
    println(f'{host[0]} {host[1]} {host[2]} {host[3]} {gpu_uploads()}')
",
        "after first launch: 1\nafter assignment: 2\n4 4 4 4 2",
    );
}
