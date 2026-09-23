// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// A `gpu`-resident binding owns one device buffer per *activation* of its
// declaration, not one per program. A recursive call re-declares the binding
// while the caller's activation is still live, and a loop re-declares it once
// per iteration; each must get a buffer of its own, and each buffer must be
// freed exactly once, when its activation leaves scope.

use super::device::require_gpu_int64;
use super::utils::*;

/// Each recursion level writes to its own buffer. With one buffer shared by
/// every activation, the inner call's `+ 10` lands on the caller's buffer too
/// and releases it, so the caller reads back a re-uploaded host copy.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn recursive_activations_each_own_a_device_buffer() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io

fn fill(depth int) float
    gpu var buf = [0.0, 0.0, 0.0, 0.0]
    forall i in 0..4
        buf[i] = 1.0
    var inner = 0.0
    if depth > 0
        inner = fill(depth - 1)
    forall i in 0..4
        buf[i] = buf[i] + 10.0
    let host = buf
    return host[0] + inner

fn main()
    println(f\"{fill(2)}\")
",
        "33.0",
    );
}

/// Every live activation reads back the value it wrote, not a sibling's, and
/// every activation's buffer is released exactly once.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn recursive_activations_read_back_their_own_values_and_release_once() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io
use system.gpu

fn level(depth int)
    gpu var buf = [0, 0, 0, 0]
    forall i in 0..4
        buf[i] = depth + 1
    if depth > 0
        level(depth - 1)
    forall i in 0..4
        buf[i] = buf[i] * 10
    let host = buf
    println(f\"{depth}:{host[0]}:{host[3]}\")

fn main()
    gpu_reset_telemetry()
    level(2)
    println(f\"releases {gpu_releases()} uploads {gpu_uploads()}\")
",
        "0:10:10\n1:20:20\n2:30:30\nreleases 3 uploads 3",
    );
}

/// A loop body that declares a `gpu var` gets a fresh buffer every iteration:
/// the host initializer is uploaded each time and the previous iteration's
/// buffer is released when its scope ends.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn loop_iterations_each_own_a_fresh_device_buffer() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io
use system.gpu

fn main()
    gpu_reset_telemetry()
    var total = 0
    var round = 0
    while round < 3
        gpu var buf = [1, 1, 1, 1]
        forall i in 0..4
            buf[i] = buf[i] + round
        let host = buf
        total = total + host[0]
        round = round + 1
    println(f\"{total} releases {gpu_releases()} uploads {gpu_uploads()}\")
",
        "6 releases 3 uploads 3",
    );
}

/// An activation left by an early `return` or a `break` is closed on that path
/// too: the caller's buffer is intact afterwards and every buffer is released
/// exactly once.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn early_exits_close_the_activation_they_leave() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io
use system.gpu

fn probe(depth int) int
    gpu var buf = [0, 0, 0, 0]
    forall i in 0..4
        buf[i] = depth + 1
    if depth == 0
        let early = buf
        return early[0]
    let inner = probe(depth - 1)
    var round = 0
    while round < 5
        gpu var scratch = [0, 0]
        forall i in 0..2
            scratch[i] = round
        round = round + 1
        if round == 2
            break
    let host = buf
    return host[0] * 10 + inner

fn main()
    gpu_reset_telemetry()
    println(f\"{probe(2)} releases {gpu_releases()}\")
",
        "51 releases 7",
    );
}

/// A reduction bound to a `gpu let` keeps its result in its own activation's
/// buffer across a recursive call that performs the same reduction.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn recursive_reduction_results_stay_with_their_activation() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io
use system.gpu

fn total(depth int) int
    gpu var xs = [depth, depth, depth, depth]
    gpu let sum = xs.reduce(0, fn(a int, b int) int: a + b)
    var inner = 0
    if depth > 0
        inner = total(depth - 1)
    let host = sum
    return host * 100 + inner

fn main()
    gpu_reset_telemetry()
    println(f\"{total(2)} releases {gpu_releases()}\")
",
        "1200 releases 6",
    );
}
