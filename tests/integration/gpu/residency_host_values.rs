// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A gpu-resident binding read into a host value is a boundary crossing
//! whatever the value is: a tuple, an array or collection literal, an operator,
//! a match or a loop all read the host array, so each has to be fenced by a
//! readback or it hands back the host array's initial values.
//!
//! Every program the suite compiles runs with `MIRI_VERIFY_MIR`, so the
//! `assert_builds` gate proves each spelling is fenced on a machine with no
//! adapter; the hardware tests prove the values.

use super::device::require_gpu_int64;
use super::utils::*;

/// Launches `buf[i] = i * 5` over a four-element gpu binding, then runs `tail`.
fn after_launch(tail: &str) -> String {
    format!(
        "
use system.io
use system.collections.array
use system.collections.list

fn main()
    gpu var buf = [0, 0, 0, 0]
    let k = 5
    gpu forall i in 0..4
        buf[i] = i * k
{tail}"
    )
}

const TUPLE_AND_ARRAY: &str = "    let t = (buf, 1)
    let h = t.0
    let xs = [buf]
    let z = xs[0]
    println(f\"{h[2]} {z[2]}\")
";

const NESTED_TUPLE: &str = "    let t = ((buf, 1), 2)
    let h = t.0.0
    println(f\"{h[2]}\")
";

const LIST_LITERAL: &str = "    let xs = List([buf])
    let h = xs[0]
    println(f\"{h[2]}\")
";

const MAP_LITERAL: &str = "    let m = {\"a\": buf}
    let h = m[\"a\"]
    println(f\"{h[2]}\")
";

/// Array equality and set membership compare arrays by identity on the host,
/// so these two read the host array without a value that shows it; the
/// verifier gate is what proves they are fenced.
const SET_LITERAL_AND_OPERATOR: &str = "    let s = {buf}
    let o = [0, 5, 10, 15]
    println(f\"{s.length()} {buf == o}\")
";

const LOOP_AND_MATCH: &str = "    var total = 0
    for x in buf
        total = total + x
    match buf
        m: println(f\"{total} {m[3]}\")
";

/// The copy is taken when the aggregate is built: a launch afterwards changes
/// the device buffer, which a later readback observes, but not the tuple's copy.
const COPY_BEFORE_A_SECOND_LAUNCH: &str = "    let t = (buf, 1)
    gpu forall i in 0..4
        buf[i] = buf[i] * 2
    let h = t.0
    println(f\"{h[2]}\")
    let h2 = buf
    println(f\"{h2[2]}\")
";

#[test]
fn every_host_value_spelling_passes_the_cross_residency_verifier() {
    for tail in [
        TUPLE_AND_ARRAY,
        NESTED_TUPLE,
        LIST_LITERAL,
        MAP_LITERAL,
        SET_LITERAL_AND_OPERATOR,
        LOOP_AND_MATCH,
        COPY_BEFORE_A_SECOND_LAUNCH,
    ] {
        assert_builds(&after_launch(tail));
    }
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn a_gpu_binding_built_into_a_tuple_or_array_carries_the_device_results() {
    require_gpu_int64();
    assert_runs_with_output(&after_launch(TUPLE_AND_ARRAY), "10 10");
    assert_runs_with_output(&after_launch(NESTED_TUPLE), "10");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn a_gpu_binding_built_into_a_collection_literal_carries_the_device_results() {
    require_gpu_int64();
    assert_runs_with_output(&after_launch(LIST_LITERAL), "10");
    assert_runs_with_output(&after_launch(MAP_LITERAL), "10");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn a_loop_and_a_match_over_a_gpu_binding_read_the_device_results() {
    require_gpu_int64();
    assert_runs_with_output(&after_launch(LOOP_AND_MATCH), "30 15");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn an_aggregate_copy_is_taken_when_the_aggregate_is_built() {
    require_gpu_int64();
    assert_runs_with_output(&after_launch(COPY_BEFORE_A_SECOND_LAUNCH), "10\n20");
}

/// A struct constructor is a call into the struct's initializer, which the
/// residency gate refuses for a gpu-resident argument rather than lowering it
/// without a readback.
#[test]
fn a_gpu_binding_passed_to_a_struct_constructor_is_refused() {
    assert_compiler_error(
        "
use system.collections.array

struct Holder
    a Array<int, 4>

fn main()
    gpu var buf = [0, 0, 0, 0]
    gpu forall i in 0..4
        buf[i] = i
    let b = Holder(a: buf)
",
        "passing gpu-resident 'buf' to function 'Holder'",
    );
}

/// A host function that reads its argument receives the host array, so the
/// residency gate refuses a gpu-resident argument to it.
#[test]
fn a_gpu_binding_passed_to_a_host_function_is_refused() {
    assert_compiler_error(
        "
use system.collections.array

fn third(a Array<int, 4>) int
    return a[2]

fn main()
    gpu var buf = [0, 0, 0, 0]
    gpu forall i in 0..4
        buf[i] = i
    let v = third(buf)
",
        "cannot pass gpu-resident 'buf' to host-only function 'third'",
    );
}

const MATCH_ARM_VALUE: &str = "    let o = [9, 9, 9, 9]
    let h = match k
        5: buf
        _: o
    println(f\"{h[2]}\")
";

/// A match arm's value is the value the match produces, so a gpu binding named
/// there is read into a host value the way a conditional's branch value is.
#[test]
fn a_match_arm_value_passes_the_cross_residency_verifier() {
    assert_builds(&after_launch(MATCH_ARM_VALUE));
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn a_match_arm_value_carries_the_device_results() {
    require_gpu_int64();
    assert_runs_with_output(&after_launch(MATCH_ARM_VALUE), "10");
}

/// `b = a` between two gpu bindings copies `a`'s value into `b`: what `a`'s
/// device buffer holds, not the host array it was declared with.
const GPU_TO_GPU_REASSIGNMENT: &str = "
use system.io
use system.collections.array

fn main()
    gpu var a = [0, 0, 0, 0]
    gpu var b = [0, 0, 0, 0]
    gpu forall i in 0..4
        a[i] = i * 5
    b = a
    gpu forall i in 0..4
        b[i] = b[i] + 1
    let h = b
    let ha = a
    println(f\"{h[2]} {ha[2]}\")
";

#[test]
fn reassigning_a_gpu_binding_from_another_passes_the_cross_residency_verifier() {
    assert_builds(GPU_TO_GPU_REASSIGNMENT);
}

/// The copy is a value: launching on `b` afterwards changes `b` alone.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn reassigning_a_gpu_binding_from_another_copies_its_device_results() {
    require_gpu_int64();
    assert_runs_with_output(GPU_TO_GPU_REASSIGNMENT, "11 10");
}

/// A host copy is taken when it is made. A later launch and readback of the
/// same binding change what the binding holds, not the earlier copy.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn a_host_copy_keeps_its_values_across_a_later_readback() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io
use system.collections.array

fn main()
    gpu var a = [0, 0, 0, 0]
    gpu forall i in 0..4
        a[i] = i * 5
    let h = a
    gpu forall i in 0..4
        a[i] = a[i] * 2
    let h2 = a
    println(f\"{h[2]} {h2[2]}\")
",
        "10 20",
    );
    assert_runs_with_output(
        "
use system.io
use system.collections.array

fn main()
    gpu var a = [0, 0, 0, 0]
    var h = [1, 1, 1, 1]
    var k = 0
    while k < 3
        if k == 1
            h = a
        gpu forall i in 0..4
            a[i] = i * 5 + k
        k = k + 1
    let z = a
    println(f\"{h[2]} {z[2]}\")
",
        "10 12",
    );
}

/// One launch, then the same binding read on every turn of a loop: the device
/// buffer changes once, so it is read back once, not once per read.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn repeated_host_reads_after_one_launch_read_back_once() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io
use system.gpu
use system.collections.array

fn main()
    gpu_reset_telemetry()
    gpu var g = [0, 0, 0, 0]
    forall i in 0..4
        g[i] = 1
    var s = 0
    for k in 0..200
        let c = g
        s = s + c[k % 4]
    println(f\"{s} {gpu_readbacks()} {gpu_fences()}\")
",
        "200 1 1",
    );
}

/// A tuple, a loop and a match reading one binding after one launch need the
/// one readback the first of them performs.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn host_reads_in_different_positions_share_one_readback() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io
use system.gpu
use system.collections.array

fn main()
    gpu_reset_telemetry()
    gpu var g = Array<int, 4>()
    forall i in 0..4
        g[i] = i + 1
    let t = (g, 1)
    let a = t.0
    var s = 0
    for x in g
        s = s + x
    match g
        c: println(f\"{a[3]} {s} {c[2]} {gpu_readbacks()}\")
",
        "4 10 3 1",
    );
}

/// A gpu scalar that a host-side reduction initialized was never launched on,
/// so its host value is its value: reading it needs no readback, and the
/// verifier must not demand one.
fn untouched_gpu_scalar(tail: &str) -> String {
    format!(
        "
use system.io
use system.collections.list

fn main()
    let xs = List([1, 2, 3, 4])
    gpu let s = xs.reduce(0, fn(a int, b int) int: a + b)
{tail}"
    )
}

#[test]
fn reading_an_untouched_gpu_scalar_passes_the_cross_residency_verifier() {
    for tail in [
        "    let y = -s\n    println(f\"{y}\")\n",
        "    let y = s as float\n    println(f\"{y}\")\n",
        "    println(f\"{s}\")\n",
    ] {
        assert_builds(&untouched_gpu_scalar(tail));
    }
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn reading_an_untouched_gpu_scalar_gives_its_host_value() {
    require_gpu_int64();
    assert_runs_with_output(
        &untouched_gpu_scalar("    let y = -s\n    println(f\"{y}\")\n"),
        "-10",
    );
    assert_runs_with_output(
        &untouched_gpu_scalar("    let y = s as float\n    println(f\"{y}\")\n"),
        "10.0",
    );
    assert_runs_with_output(&untouched_gpu_scalar("    println(f\"{s}\")\n"), "10");
}

/// A reduction over a gpu binding into a gpu scalar leaves the sum on the
/// device; every host read of the scalar reads it back first.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn a_device_reduction_into_a_gpu_scalar_reads_back_for_every_host_read() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io
use system.collections.array

fn main()
    gpu var xs = [1, 2, 3, 4]
    gpu let s = xs.reduce(0, fn(a int, b int) int: a + b)
    println(f\"{s}\")
    let y = -s
    println(f\"{y}\")
",
        "10\n-10",
    );
}
