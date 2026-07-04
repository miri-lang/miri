// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// Slice readback (`g.slice(0..N)`) on a gpu-resident binding:
//   * triggers a partial readback — a host `List<T>` of the sliced range.
//   * the element-cross-read prohibition (D22) still holds: `g[0]` from host
//     is rejected, but `g.slice(...)` is the sanctioned bulk peek.
//   * the readback is a copy — the gpu binding survives and may be sliced or
//     read back again.

use super::device::gpu_int64_available;
use super::utils::*;

#[test]
fn slice_of_gpu_binding_type_checks() {
    assert_type_checks(
        "
use system.collections.array
use system.collections.list

fn main()
    gpu var arr = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i in 0..8
        arr[i] = i * i

    let h = arr.slice(0..4)
    let v = h[0]
",
    );
}

#[test]
fn slice_result_length_is_range_len() {
    assert_type_checks(
        "
use system.collections.array
use system.collections.list

fn main()
    gpu var arr = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i in 0..8
        arr[i] = i

    let h = arr.slice(2..6)
    let n = h.length()
",
    );
}

#[test]
fn slice_does_not_consume_gpu_binding() {
    // A slice is a readback copy — the binding survives, so a second slice and
    // a full readback are both valid afterward.
    assert_type_checks(
        "
use system.collections.array
use system.collections.list

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
        arr[i] = i * i

    let h = arr.slice(0..2)
    let h2 = arr.slice(0..2)
    let full = arr
",
    );
}

#[test]
fn slice_end_out_of_bounds_is_rejected() {
    assert_compiler_error(
        "
use system.collections.array
use system.collections.list

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
        arr[i] = i

    let h = arr.slice(0..10)
",
        "out of bounds",
    );
}

#[test]
fn slice_negative_start_is_rejected() {
    assert_compiler_error(
        "
use system.collections.array
use system.collections.list

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
        arr[i] = i

    let h = arr.slice(-1..2)
",
        "non-negative",
    );
}

#[test]
fn slice_reversed_range_is_rejected() {
    assert_compiler_error(
        "
use system.collections.array
use system.collections.list

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
        arr[i] = i

    let h = arr.slice(3..1)
",
        "greater than end",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn slice_reads_back_computed_values() {
    if !gpu_int64_available() {
        eprintln!("[gpu] skipped slice_reads_back_computed_values: no suitable adapter");
        return;
    }
    // arr[i] = i*i on the device; slice [2..5) peeks {4, 9, 16}.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.list

fn main()
    gpu var arr = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i in 0..8
        arr[i] = i * i

    let h = arr.slice(2..5)
    println(f\"{h[0]} {h[1]} {h[2]}\")
",
        "4 9 16",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn slice_leaves_gpu_binding_readable() {
    if !gpu_int64_available() {
        eprintln!("[gpu] skipped slice_leaves_gpu_binding_readable: no suitable adapter");
        return;
    }
    // The binding survives the slice: a subsequent full readback still works.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.list

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
        arr[i] = i * i

    let peek = arr.slice(0..2)
    let full = arr
    println(f\"{peek[1]} {full[3]}\")
",
        "1 9",
    );
}
