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
/// survives it: a declaring read afterwards answers with the same values, and
/// with no launch between the two it finds the host array current and costs
/// no second readback.
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
        "9 1\n9 1",
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

/// Returning a gpu binding hands the caller its device results. The return
/// edge is a host boundary like `let h = g`: the return slot is a host local,
/// and the binding's device buffer is released on the way out, so a return
/// that skips the readback hands back the host array's initial values. Every
/// return spelling is covered — an early return from a branch, one from inside
/// a loop, a trailing `return`, and an implicit last-expression return.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn returning_a_gpu_binding_reads_back_the_device_results() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.collections.array

fn pick(k int) Array<int, 4>
    gpu var b = [0, 0, 0, 0]
    forall i in 0..4
        b[i] = 9 + k
    if k > 1
        return b
    var i = 0
    while i < 3
        if k == 1
            return b
        i = i + 1
    return b

fn implicit() Array<int, 4>
    gpu var b = [0, 0, 0, 0]
    forall i in 0..4
        b[i] = 7
    b

fn main()
    let early = pick(2)
    let in_loop = pick(1)
    let trailing = pick(0)
    let tail = implicit()
    println(f\"{early[0]} {in_loop[1]} {trailing[2]} {tail[3]}\")
",
        "11 10 9 7",
    );
}

/// A gpu-resident reduction result returned from a function carries the
/// device's value, not the output buffer's host-side seed.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn returning_a_gpu_scalar_reads_back_the_device_result() {
    require_gpu_int64();
    assert_runs_with_output(
        "
fn total() int
    gpu var data = [1, 2, 3, 4]
    gpu let sum = data.reduce(0, fn(a int, b int) int: a + b)
    return sum

fn main()
    println(f\"{total()}\")
",
        "10",
    );
}

/// The verifier gate for the return edge: building runs `MIRI_VERIFY_MIR`, which
/// refuses a copy of a gpu binding into the host return slot that no readback
/// fences. Covers the class on a machine with no adapter.
#[test]
fn returning_a_gpu_binding_passes_the_cross_residency_verifier() {
    assert_builds(
        "
use system.collections.array

fn explicit() Array<int, 4>
    gpu var b = [0, 0, 0, 0]
    forall i in 0..4
        b[i] = 9
    return b

fn implicit() Array<int, 4>
    gpu var b = [0, 0, 0, 0]
    forall i in 0..4
        b[i] = 9
    b

fn main()
    let e = explicit()
    let i = implicit()
    println(f\"{e.length()} {i.length()}\")
",
    );
}

/// A closure captures by value at its creation (SPEC: captures are copies taken
/// when the closure is created). Capturing a gpu binding is therefore a host
/// boundary: the capture is fenced, so the closure sees the device's results as
/// of that point rather than the host array's initial values. Both closure
/// spellings — a lambda and a nested `fn` — capture the same way.
///
/// The closures run before the host readback of `a`: that readback writes the
/// device buffer into the host array the capture shares, which would supply
/// the value after the fact and hide a missing capture fence.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn closure_capture_of_a_gpu_binding_sees_the_device_results() {
    require_gpu_int64();
    assert_runs_with_output(
        "
fn main()
    gpu var a = [1, 2, 3, 4]
    forall i in 0..4
        a[i] = a[i] * 5
    let g = fn() int
        let c = a
        return c[0]
    fn last() int
        let c = a
        return c[3]
    let r = g()
    let l = last()
    let h = a
    println(f\"{h[0]} {r} {l}\")
",
        "5 5 20",
    );
}

/// The captured copy is taken when the closure is created: a launch after the
/// capture changes the device buffer, which a later host readback observes, but
/// not the closure's copy.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn closure_capture_of_a_gpu_binding_is_a_copy_taken_at_creation() {
    require_gpu_int64();
    assert_runs_with_output(
        "
fn main()
    gpu var a = [1, 2, 3, 4]
    forall i in 0..4
        a[i] = a[i] * 5
    let g = fn() int
        let c = a
        return c[0]
    forall i in 0..4
        a[i] = a[i] * 2
    let r = g()
    let h = a
    println(f\"{h[0]} {r}\")
",
        "10 5",
    );
}

/// A gpu scalar no launch has touched has no device buffer, so its readback has
/// nothing to copy: the host value it was declared with is the answer.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_scalar_with_no_device_buffer_reads_back_its_declared_value() {
    require_gpu_int64();
    assert_runs_with_output(
        "
fn main()
    gpu let s = 7
    let h = s
    gpu var f = 2.5
    let hf = f
    println(f\"{h} {hf}\")
",
        "7 2.5",
    );
}

/// A reduction assigned into an existing gpu scalar reads back as the device's
/// result.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn reduction_assigned_into_a_gpu_scalar_reads_back() {
    require_gpu_int64();
    assert_runs_with_output(
        "
fn main()
    gpu var xs = [1, 2, 3, 4]
    gpu var s = 7
    s = xs.reduce(0, fn(a int, b int) int: a + b)
    let h = s
    println(f\"{h}\")
",
        "10",
    );
}

/// The return and capture fences each cost exactly one readback, and every
/// buffer they read is still released exactly once when its binding's function
/// returns: one upload, one readback and one release per binding.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn return_and_capture_fences_keep_device_telemetry_balanced() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

fn make() Array<int, 4>
    gpu var b = [0, 0, 0, 0]
    forall i in 0..4
        b[i] = 9
    return b

fn capture() int
    gpu var a = [1, 2, 3, 4]
    forall i in 0..4
        a[i] = a[i] * 5
    let g = fn() int
        let c = a
        return c[0]
    return g()

fn main()
    gpu_reset_telemetry()
    let h = make()
    let r = capture()
    println(f\"{h[0]} {r} uploads {gpu_uploads()} readbacks {gpu_readbacks()} releases {gpu_releases()}\")
",
        "9 5 uploads 2 readbacks 2 releases 2",
    );
}

/// The verifier gate for captures: building runs `MIRI_VERIFY_MIR`, which
/// refuses a closure that captures a gpu binding no readback fences. Covers
/// both closure spellings on a machine with no adapter.
#[test]
fn capturing_a_gpu_binding_passes_the_cross_residency_verifier() {
    assert_builds(
        "
fn main()
    gpu var a = [1, 2, 3, 4]
    forall i in 0..4
        a[i] = a[i] * 5
    let g = fn() int
        let c = a
        return c[0]
    fn last() int
        let c = a
        return c[3]
    println(f\"{g()} {last()}\")
",
    );
}
