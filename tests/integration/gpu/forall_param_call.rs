// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// Test Slice 2 of F12: per-residency device-handle Call-ABI.
// Functions containing `forall` bodies that index params can accept gpu-resident args
// if the buffer access only occurs inside the forall (device-side context).

use super::utils::*;

#[test]
fn scale_gpu_param_via_forall() {
    // RED: This must FAIL today because scale is classified HostOnly.
    // GREEN: After Slice 2, this will compile because scale is GpuLaunchSafe.
    // The forall body indexes the param, but only in device context (inside forall).
    // The type checker must allow this and emit a device-handle ABI call.
    assert_type_checks(
        "
use system.gpu
use system.collections.array

fn scale(a out Array<int,8>)
    forall i in 0..a.length()
        a[i] = a[i] * 2

fn main()
    gpu var data = [1, 2, 3, 4, 5, 6, 7, 8]
    scale(data)
",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn scale_gpu_param_via_forall_readback() {
    // This tests the FULL behavior: pass gpu-resident buffer to a function
    // that launches a kernel on it, mutate in-place, verify by readback.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

fn scale(a out Array<int,8>)
    forall i in 0..a.length()
        a[i] = a[i] * 2

fn main()
    gpu var data = [1, 2, 3, 4, 5, 6, 7, 8]
    scale(data)
    let host = data
    println(f\"{host[0]} {host[1]} {host[2]} {host[3]} {host[4]} {host[5]} {host[6]} {host[7]}\")
",
        "2 4 6 8 10 12 14 16",
    );
}

#[test]
fn host_access_on_gpu_param_still_rejected() {
    // CRITICAL: Host-context buffer access on a gpu param must still be rejected.
    // This ensures we don't regress the D22 fix (no per-element host readback).
    // A function with `a[0]` at host scope is HostOnly and rejects gpu args.
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn get_first(a Array<int,8>) int
    return a[0]

fn main()
    gpu var data = [1, 2, 3, 4, 5, 6, 7, 8]
    let x = get_first(data)
",
        "cannot pass gpu-resident",
    );
}

#[test]
fn mixed_host_and_forall_access_rejected() {
    // CRITICAL: If a function mixes host-context and forall-context access,
    // the host-context access wins and makes it HostOnly.
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn mixed(a out Array<int,8>)
    let x = a[0]
    forall i in 0..a.length()
        a[i] = a[i] * 2

fn main()
    gpu var data = [1, 2, 3, 4, 5, 6, 7, 8]
    mixed(data)
",
        "cannot pass gpu-resident",
    );
}

#[test]
fn forall_with_non_param_access() {
    // A forall that doesn't touch the param is still polymorphic-safe.
    // (Just calling the function with no param access in forall should work)
    assert_type_checks(
        "
use system.gpu
use system.collections.array

fn check_len(a Array<int,4>) int
    return a.length()

fn main()
    let h = [1, 2, 3, 4]
    gpu var g = [5, 6, 7, 8]
    let _ = check_len(h)
    let _ = check_len(g)
",
    );
}

#[test]
fn forall_with_host_intrinsic_still_rejected() {
    // If the forall body calls a host-forcing intrinsic (like println),
    // the function is HostOnly and rejects gpu args.
    assert_compiler_error(
        "
use system.gpu
use system.collections.array
use system.io

fn debug_in_forall(a out Array<int,4>)
    forall i in 0..a.length()
        println(f\"{a[i]}\")

fn main()
    gpu var g = [1, 2, 3, 4]
    debug_in_forall(g)
",
        "host-only",
    );
}

#[test]
fn forwarding_param_still_rejected() {
    // Passing a param to another function is buffer-touching, even if
    // the callee only does forall access. This is out of scope for Slice 2.
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn scale(a out Array<int,4>)
    forall i in 0..a.length()
        a[i] = a[i] * 2

fn caller(a Array<int,4>)
    scale(a)

fn main()
    gpu var g = [1, 2, 3, 4]
    caller(g)
",
        "buffer-touching",
    );
}

#[test]
fn returning_param_still_rejected() {
    // Returning a param is buffer-touching (aliasing).
    assert_compiler_error(
        "
use system.gpu
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn two_gpu_params_one_written_one_read() {
    // Two gpu-resident buffers reach one callee: `a` is written in-place from
    // `b` (read-only). Exercises multi-handle residency mangling and stamping.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

fn add_into(a out Array<int,4>, b Array<int,4>)
    forall i in 0..a.length()
        a[i] = a[i] + b[i]

fn main()
    gpu var x = [1, 2, 3, 4]
    gpu var y = [10, 20, 30, 40]
    add_into(x, y)
    let hx = x
    let hy = y
    println(f\"{hx[0]} {hx[1]} {hx[2]} {hx[3]} {hy[0]} {hy[3]}\")
",
        "11 22 33 44 10 40",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn same_gpu_fn_called_twice_reuses_buffer() {
    // The same GpuLaunchSafe fn called twice on the same gpu buffer: one
    // specialization (dedup), persistent buffer reused across launches.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

fn scale(a out Array<int,4>)
    forall i in 0..a.length()
        a[i] = a[i] * 2

fn main()
    gpu var data = [1, 2, 3, 4]
    scale(data)
    scale(data)
    let host = data
    println(f\"{host[0]} {host[1]} {host[2]} {host[3]}\")
",
        "4 8 12 16",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn polymorphic_fn_called_with_host_then_gpu() {
    // A PolymorphicSafe fn (only `.length()`, no buffer touch) called once with
    // a host array and once with a gpu-resident buffer in the same program.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

fn check_len(a Array<int,4>) int
    return a.length()

fn main()
    let h = [1, 2, 3, 4]
    gpu var g = [5, 6, 7, 8]
    let lh = check_len(h)
    let lg = check_len(g)
    println(f\"{lh} {lg}\")
",
        "4 4",
    );
}
