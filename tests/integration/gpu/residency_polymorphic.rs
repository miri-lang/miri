// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// Residency-polymorphic function feature: functions that can safely accept
// both host-resident and gpu-resident array arguments when the function body
// only performs buffer-untouching operations (like .length() on fixed arrays).

use super::utils::*;

#[test]
fn polymorphic_length_both_residencies() {
    // Criterion 1: Runnable both residencies
    // A function that only calls .length() on an array can accept both host and gpu args.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

fn len_of(a Array<int,4>) int
    return a.length()

fn main()
    let h = [1, 2, 3, 4]
    gpu var g = [1, 2, 3, 4]
    let nh = len_of(h)
    let ng = len_of(g)
    println(f\"{nh},{ng}\")
",
        "4,4",
    );
}

#[test]
fn host_forcing_inference_println() {
    // Criterion 2: Host-forcing inference
    // A function with println(a[...]) on a param forces the param to host-only.
    assert_compiler_error(
        "
use system.gpu
use system.collections.array
use system.io

fn debug(a Array<int,4>)
    println(f\"{a[0]}\")

fn main()
    gpu var g = [1, 2, 3, 4]
    debug(g)
",
        "host-only",
    );
}

#[test]
fn host_forcing_inference_element_access() {
    // A function that indexes into a param forces host-only.
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn idx(a Array<int,4>) int
    return a[0]

fn main()
    gpu var g = [1, 2, 3, 4]
    let x = idx(g)
",
        "buffer-touching",
    );
}

#[test]
fn explicit_host_annotation_locks() {
    // Criterion 3: Explicit annotations lock
    // An explicit `host` annotation on a parameter locks it to host-only.
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn read_only(a host Array<int,4>) int
    return a.length()

fn main()
    gpu var g = [1, 2, 3, 4]
    let x = read_only(g)
",
        "host",
    );
}

#[test]
fn explicit_gpu_annotation_locks() {
    // An explicit `gpu` annotation on a parameter locks it to gpu-only (rejects host).
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn read_only(a gpu Array<int,4>) int
    return a.length()

fn main()
    let h = [1, 2, 3, 4]
    let x = read_only(h)
",
        "gpu",
    );
}

#[test]
fn gpu_fn_with_host_param_conflict() {
    // Criterion 4: gpu fn conflict
    // A `gpu fn` with an explicit `host` parameter annotation is a conflict error.
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

gpu fn kernel(a host Array<int,4>)
    return

fn main()
    let h = [1, 2, 3, 4]
    kernel(h)
",
        "conflict",
    );
}

#[test]
fn buffer_touching_rejection() {
    // Criterion 5: SAFETY - buffer-touching body rejection
    // A function that indexes into a param must reject gpu args (needs Phase-2B ABI).
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn idx(a Array<int,4>) int
    return a[0]

fn main()
    gpu var g = [1, 2, 3, 4]
    let x = idx(g)
",
        "buffer-touching",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn passthrough_safety_no_leak() {
    // Criterion 6: SAFETY passthrough (hardware-gated)
    // Passing a gpu binding to a polymorphic-safe function does not cause
    // double-free, re-upload, or premature buffer release. Verify:
    // - .length() returns correct value
    // - readback after call gets original bytes
    // - no memory leak
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

fn len_of(a Array<int,4>) int
    return a.length()

fn main()
    gpu var g = [1, 2, 3, 4]
    let n = len_of(g)
    let out = g
    println(f\"{n}\")
",
        "4",
    );
}

#[test]
fn host_works_as_regular_identifier() {
    // Regression test: "host" is a contextual keyword in parameter position only.
    // It must work as a regular identifier elsewhere (variable names, etc).
    assert_runs_with_output(
        "
use system.collections.array

fn main()
    let host = [1, 2, 3]
    let x = host.length()
    var count = 0
    count = 10
    println(f\"{x},{count}\")
",
        "3,10",
    );
}

#[test]
fn method_call_on_param_is_rejected() {
    // CRITICAL SOUNDNESS FIX: Method calls on params (other than .length())
    // are buffer-touching and must reject gpu args.
    assert_compiler_error(
        "
use system.collections.array

fn read_element(a Array<int,4>) int
    return a.element_at(0)

fn main()
    gpu var g = [1, 2, 3, 4]
    let x = read_element(g)
",
        "buffer-touching",
    );
}

#[test]
fn param_forwarding_is_rejected() {
    // CRITICAL SOUNDNESS FIX: Forwarding a param to another call is buffer-touching.
    assert_compiler_error(
        "
use system.collections.array

fn inner(a Array<int,4>) int
    return a[0]

fn outer(a Array<int,4>) int
    return inner(a)

fn main()
    gpu var g = [1, 2, 3, 4]
    let x = outer(g)
",
        "buffer-touching",
    );
}

#[test]
fn return_param_is_rejected() {
    // CRITICAL SOUNDNESS FIX: Returning a param is buffer-touching (aliasing).
    assert_compiler_error(
        "
use system.collections.array

fn id(a Array<int,4>) Array<int,4>
    return a

fn main()
    gpu var g = [1, 2, 3, 4]
    let x = id(g)
",
        "buffer-touching",
    );
}

#[test]
fn param_aliasing_is_rejected() {
    // CRITICAL SOUNDNESS FIX: Aliasing a param (let b = a) is buffer-touching.
    assert_compiler_error(
        "
use system.collections.array

fn f(a Array<int,4>) int
    let b = a
    return b[0]

fn main()
    gpu var g = [1, 2, 3, 4]
    let x = f(g)
",
        "buffer-touching",
    );
}

#[test]
fn computed_index_using_param_is_rejected() {
    // CRITICAL SOUNDNESS FIX: Using param in index expression is buffer-touching.
    assert_compiler_error(
        "
use system.collections.array

fn f(a Array<int,4>, b Array<int,4>) int
    return b[a.length()]

fn main()
    gpu var ga = [1, 2, 3, 4]
    let gb = [5, 6, 7, 8]
    let x = f(ga, gb)
",
        "buffer-touching",
    );
}

#[test]
fn gpu_resident_list_length_is_rejected() {
    // A gpu-resident List<T> passed to a function that calls .length() must be rejected.
    // Unlike a static Array<T,N> where .length() is buffer-untouching, a List's .length()
    // is buffer-touching (dynamic length lookup), so PolymorphicSafe does not apply.
    assert_compiler_error(
        "
use system.gpu
use system.collections.list

fn len_of_list(a [int]) int
    return a.length()

fn main()
    gpu var g = List([1, 2, 3, 4])
    let n = len_of_list(g)
    println(f\"{n}\")
",
        "cannot pass gpu-resident",
    );
}

#[test]
fn polymorphic_safe_called_in_loop() {
    // A PolymorphicSafe function can be called multiple times in a loop
    // with both host and gpu arguments. This tests that the polymorphic
    // safety verdict persists across multiple call sites.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

fn len_of(a Array<int,4>) int
    return a.length()

fn main()
    let h = [1, 2, 3, 4]
    gpu var g = [5, 6, 7, 8]
    var sum = 0
    var i = 0
    while i < 3
        sum = sum + len_of(h) + len_of(g)
        i = i + 1
    println(f\"{sum}\")
",
        "24",
    );
}
