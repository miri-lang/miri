// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// Cross-residency assignment rules:
//   * host → gpu (`gpu let g = host_x`) and gpu → host (`let h = gpu_g`) are
//     copies — the source survives.
//   * gpu → gpu (`gpu let b = gpu_a`) is a linear move — `gpu_a` is consumed.
//   * element cross-read (`let v = gpu_g[0]`) is rejected.
//   * passing a gpu-resident value to a host call (`println(gpu_g)`) is
//     rejected.

use super::device::require_gpu_int64;
use super::utils::*;

#[test]
fn element_cross_read_from_host_is_rejected() {
    assert_compiler_error(
        "
use system.collections.array

fn main()
    gpu var arr = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i in 0..8
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
    gpu forall i in 0..4
        arr[i] = i * i

    let v = arr[0]
",
        "let h = arr",
    );
}

#[test]
fn method_element_at_cross_read_from_host_is_rejected() {
    assert_compiler_error(
        "
use system.collections.array

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
        arr[i] = i * i

    let v = arr.element_at(1)
",
        "cannot call method 'element_at' on gpu-resident",
    );
}

#[test]
fn method_contains_cross_read_from_host_is_rejected() {
    assert_compiler_error(
        "
use system.collections.array

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
        arr[i] = i * i

    let found = arr.contains(10)
",
        "cannot call method 'contains' on gpu-resident",
    );
}

#[test]
fn method_index_of_cross_read_from_host_is_rejected() {
    assert_compiler_error(
        "
use system.collections.array

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
        arr[i] = i * i

    let idx = arr.index_of(9)
",
        "cannot call method 'index_of' on gpu-resident",
    );
}

#[test]
fn method_set_host_write_is_rejected() {
    assert_compiler_error(
        "
use system.collections.array

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
        arr[i] = i * i

    arr.set(0, 99)
",
        "cannot call method 'set' on gpu-resident",
    );
}

#[test]
fn method_cross_read_diagnostic_proposes_readback_fixit() {
    assert_compiler_error(
        "
use system.collections.array

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
        arr[i] = i * i

    let v = arr.element_at(0)
",
        "let h = arr",
    );
}

#[test]
fn method_length_on_gpu_resident_is_allowed() {
    // `.length()` reads only compile-time array metadata, never the buffer, so
    // it stays legal from host context (whitelisted alongside slice/reduce).
    assert_type_checks(
        "
use system.collections.array

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
        arr[i] = i * i

    let n = arr.length()
",
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
    gpu forall i in 0..4
        dst[i] = a[i] + 1
",
    );
}

#[test]
fn println_gpu_resident_is_rejected() {
    assert_compiler_error(
        "
use system.collections.array

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
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
    gpu forall i in 0..4
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
    gpu forall i in 0..4
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
    gpu forall i in 0..4
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vector_add_demo_value_correctness() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.gpu

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu let b = [5.0, 6.0, 7.0, 8.0]
    gpu var dst = [0.0, 0.0, 0.0, 0.0]

    gpu forall i in 0..4
        dst[i] = a[i] + b[i]

    let host = dst
    println(f\"{host[0]} {host[1]} {host[2]} {host[3]}\")
",
        "6.0 8.0 10.0 12.0",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn two_readbacks_produce_independent_host_arrays() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.gpu

fn main()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
        arr[i] = i * i

    let h = arr
    let h2 = arr
    println(f\"{h[3]} {h2[3]}\")
",
        "9 9",
    );
}
