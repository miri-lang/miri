// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A `GpuLaunchSafe` function called with different buffer-residency patterns
//! is lowered once per pattern, and each lowering emits the closures written in
//! its body again. Those copies need distinct symbols.
//!
//! Closures are told apart by the generic substitution and the receiver. A
//! residency specialization has neither — it is a free function and no generic
//! is substituted — so the residency pattern itself has to reach the symbol,
//! or the second copy is refused as a duplicate definition.

use super::device::assert_gpu_runs_with_output;
use super::utils::*;

#[test]
fn two_residency_specializations_emit_distinct_lambda_symbols() {
    assert_builds(
        r#"
use system.collections.array

fn scale(a out Array<int,8>) int
    let bump = fn(x int) int: x + 1
    forall i in 0..a.length()
        a[i] = a[i] * 2
    return bump(1)

fn main()
    gpu var device = [1, 2, 3, 4, 5, 6, 7, 8]
    var host = [1, 2, 3, 4, 5, 6, 7, 8]
    let a = scale(device)
    let b = scale(host)
    println(f"{a} {b}")
"#,
    );
}

/// Two gpu-resident buffers reach the same collision one symbol along: each
/// specialization carries the `forall`'s kernel, so the kernel's name carries
/// the residency pattern too, and each buffer is doubled by its own kernel.
#[test]
fn two_gpu_resident_buffers_through_one_function_emit_distinct_kernels() {
    assert_gpu_runs_with_output(
        r#"
use system.collections.array

fn scale(a out Array<int,8>) int
    let bump = fn(x int) int: x + 1
    forall i in 0..a.length()
        a[i] = a[i] * 2
    return bump(1)

fn main()
    gpu var first = [1, 2, 3, 4, 5, 6, 7, 8]
    gpu var second = [9, 9, 9, 9, 9, 9, 9, 10]
    let a = scale(first)
    let b = scale(second)
    let one = first
    let two = second
    println(f"{a} {b} {one[0]} {one[7]} {two[0]} {two[7]}")
"#,
        "2 2 2 16 18 20",
    );
}

/// A nested function in the same position is emitted the same way, under the
/// same naming, so it collides identically.
#[test]
fn two_residency_specializations_emit_distinct_nested_function_symbols() {
    assert_builds(
        r#"
use system.collections.array

fn scale(a out Array<int,8>) int
    fn bump(x int) int
        return x + 1
    forall i in 0..a.length()
        a[i] = a[i] * 2
    return bump(1)

fn main()
    gpu var device = [1, 2, 3, 4, 5, 6, 7, 8]
    var host = [1, 2, 3, 4, 5, 6, 7, 8]
    let a = scale(device)
    let b = scale(host)
    println(f"{a} {b}")
"#,
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn both_residency_specializations_compute_their_own_answer() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn scale(a out Array<int,8>) int
    let bump = fn(x int) int: x + 1
    forall i in 0..a.length()
        a[i] = a[i] * 2
    return bump(1)

fn main()
    gpu var device = [1, 2, 3, 4, 5, 6, 7, 8]
    var host = [1, 2, 3, 4, 5, 6, 7, 8]
    let a = scale(device)
    let b = scale(host)
    let back = device
    println(f"{a} {b} {back[0]} {back[7]} {host[0]} {host[7]}")
"#,
        "2 2 2 16 2 16",
    );
}
