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

/// The device has no tagged-union representation, so a call returning an
/// optional cannot be lowered for it. The type checker has to say so against the
/// offending line: reaching the backend instead yields an internal error naming
/// a MIR aggregate kind, which does not identify the program that caused it.
#[test]
fn optional_returning_method_in_device_code_is_rejected() {
    assert_compiler_error(
        "
use system.collections.array

fn main()
    gpu var arr = [1, 2, 3, 4]
    gpu forall i in 0..4
        let idx = arr.index_of(2) ?? 0
        arr[i] = idx
",
        "cannot be represented in device code",
    );
}

/// A readback into an already-declared binding (`h = g`) transfers the device
/// buffer exactly as the declaring spelling (`let h = g`) does. The assigned
/// binding is read *before* the declaring spelling runs: a readback into the
/// shared host buffer would otherwise repair the assignment after the fact and
/// make a missing transfer look correct.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn assignment_into_existing_var_reads_back_like_a_declaration() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.collections.array

gpu fn fill(dst out Array<int, 4>)
    dst[kernel.global_idx.x] = kernel.global_idx.x + 99

fn main()
    gpu var out = Array<int, 4>()
    fill(out).launch(Dim3(4, 1, 1), Dim3(1, 1, 1))

    var assigned = Array<int, 4>()
    assigned = out
    println(f\"{assigned[0]} {assigned[1]} {assigned[2]} {assigned[3]}\")

    let declared = out
    println(f\"{declared[0]} {declared[1]} {declared[2]} {declared[3]}\")
",
        "99 100 101 102\n99 100 101 102",
    );
}

/// The assignment spelling pays exactly one readback, and the gpu binding
/// survives it: a declaring readback afterwards answers with the same values
/// and costs a second one.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn assignment_readback_leaves_the_gpu_binding_readable() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.gpu

fn main()
    gpu_reset_telemetry()
    gpu var arr = [0, 0, 0, 0]
    gpu forall i in 0..4
        arr[i] = i * i

    var h = [0, 0, 0, 0]
    h = arr
    println(f\"{h[3]} {gpu_readbacks()}\")

    let h2 = arr
    println(f\"{h2[3]} {gpu_readbacks()}\")
",
        "9 1\n9 2",
    );
}

/// A gpu-resident scalar assigned into an existing host `var` is read back the
/// same way a declaration reads it, and is read before the declaring spelling
/// runs.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn scalar_assignment_into_existing_var_reads_back() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [1, 2, 3, 4]
    gpu let total = data.reduce(0, fn(a i32, b i32) i32: a + b)

    var assigned = 0
    assigned = total
    println(f\"{assigned}\")

    let declared = total
    println(f\"{declared}\")
",
        "10\n10",
    );
}

/// The class gate. Every program the suite compiles runs with `MIRI_VERIFY_MIR`
/// findings fatal, and the verifier now refuses any copy of a gpu binding into
/// a host binding that no readback fences — whichever spelling emitted it. A
/// lowering path added later that forgets the fence fails the build rather than
/// shipping as a silent no-op. Building is enough to run the check, so this
/// covers the class on a machine with no adapter.
#[test]
fn both_readback_spellings_pass_the_cross_residency_verifier() {
    assert_builds(
        "
use system.collections.array

fn main()
    gpu var g = [0, 0, 0, 0]
    gpu forall i in 0..4
        g[i] = i * i

    var assigned = [0, 0, 0, 0]
    assigned = g
    let declared = g
    println(f\"{assigned.length()} {declared.length()}\")
",
    );
}
