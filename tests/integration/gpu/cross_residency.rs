// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// Cross-residency assignment surface (GPU_DRAFT §6.3–§6.5, §7.1, §7.3, §16):
//   * host → gpu (`gpu let g = host_x`) and gpu → host (`let h = gpu_g`) are
//     copies — the source survives (D23).
//   * gpu → gpu (`gpu let b = gpu_a`) is a linear move — `gpu_a` is consumed
//     (D24).
//   * element cross-read (`let v = gpu_g[0]`) is rejected (D22).
//   * passing a gpu-resident value to a host call (`println(gpu_g)`) is
//     rejected (§6.4).

use super::device::gpu_adapter_available;
use super::utils::*;

#[test]
fn element_cross_read_from_host_is_rejected() {
    assert_compiler_error(
        "
use system.collections.array
use system.io

fn main()
    gpu var arr = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu for i in 0..8
        arr[i] = i * i

    for i in 0..8
        let v = arr[i]
        println(f\"{v}\")
",
        "a per-element read would require a readback",
    );
}

#[test]
fn element_cross_read_diagnostic_proposes_bulk_copy_fixit() {
    assert_compiler_error(
        "
use system.collections.array

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu for i in 0..4
        arr[i] = i * i

    let v = arr[0]
",
        "let h = arr",
    );
}

#[test]
fn host_element_read_is_allowed() {
    assert_type_checks(
        "
use system.collections.array

fn main()
    let host = [1, 2, 3, 4]
    let v = host[0]
",
    );
}

#[test]
fn kernel_body_element_read_is_allowed() {
    assert_type_checks(
        "
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = a[i] + 1
",
    );
}

#[test]
fn println_gpu_resident_is_rejected() {
    assert_compiler_error(
        "
use system.collections.array
use system.io

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu for i in 0..4
        arr[i] = i
    println(arr)
",
        "cannot pass gpu-resident 'arr' to host function",
    );
}

#[test]
fn gpu_to_gpu_assignment_consumes_source() {
    assert_compiler_error(
        "
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = a
    gpu var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = a[i] + b[i]
",
        "consumed",
    );
}

#[test]
fn gpu_to_gpu_assignment_transfers_ownership_to_target() {
    assert_type_checks(
        "
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = a
    gpu var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = b[i] + 1
",
    );
}

#[test]
fn readback_does_not_consume_gpu_binding() {
    assert_type_checks(
        "
use system.collections.array

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu for i in 0..4
        arr[i] = i * i

    let h = arr
    let h2 = arr
",
    );
}

#[test]
fn upload_from_host_value_does_not_consume_source() {
    assert_type_checks(
        "
use system.collections.array

fn main()
    let host_x = [1, 2, 3, 4]
    gpu let g = host_x
    let still_host = host_x
",
    );
}

#[test]
fn vector_add_demo_compiles_and_runs() {
    // §16.1 — compiles end-to-end on the residency surface: `gpu let` inputs,
    // a `gpu var` output, and a readback (`let host = dst`). Value correctness
    // is asserted by `vector_add_demo_value_correctness`.
    assert_runs(
        "
use system.gpu
use system.io

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu let b = [5.0, 6.0, 7.0, 8.0]
    gpu var dst = [0.0, 0.0, 0.0, 0.0]

    gpu for i in 0..4
        dst[i] = a[i] + b[i]

    let host = dst
    println(f\"{host[0]} {host[1]} {host[2]} {host[3]}\")
",
    );
}

#[test]
fn two_readbacks_compile() {
    // §16.7 `good()` shape extended: two independent host copies of the same
    // gpu binding. The binding survives both readbacks.
    assert_runs(
        "
use system.gpu
use system.io

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu for i in 0..4
        arr[i] = i * i

    let h = arr
    let h2 = arr
    println(f\"{h[3]} {h2[3]}\")
",
    );
}

#[test]
fn vector_add_demo_value_correctness() {
    if !gpu_adapter_available() {
        eprintln!("[gpu] skipped vector_add_demo_value_correctness: no suitable adapter");
        return;
    }
    assert_runs_with_output(
        "
use system.gpu
use system.io

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu let b = [5.0, 6.0, 7.0, 8.0]
    gpu var dst = [0.0, 0.0, 0.0, 0.0]

    gpu for i in 0..4
        dst[i] = a[i] + b[i]

    let host = dst
    println(f\"{host[0]} {host[1]} {host[2]} {host[3]}\")
",
        "6.0 8.0 10.0 12.0",
    );
}

#[test]
fn two_readbacks_produce_independent_host_arrays() {
    if !gpu_adapter_available() {
        eprintln!(
            "[gpu] skipped two_readbacks_produce_independent_host_arrays: no suitable adapter"
        );
        return;
    }
    assert_runs_with_output(
        "
use system.gpu
use system.io

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu for i in 0..4
        arr[i] = i * i

    let h = arr
    let h2 = arr
    println(f\"{h[3]} {h2[3]}\")
",
        "9 9",
    );
}
