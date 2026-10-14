// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// A `continue` inside a loop in a kernel body. Every `continue` and the end of
// the loop body reach the loop header through one latch block, which is the
// shape the WGSL structurizer lowers to a `loop { ... continuing { ... } }`.

use super::device::require_gpu_int64;
use super::utils::*;

/// A `while` whose body `continue`s past some iterations counts only the rest:
/// element `i` holds the number of odd values in `0..i`.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn while_with_continue_in_a_forall_body_skips_iterations() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io

fn main()
    gpu var data = [0, 0, 0, 0, 0, 0]
    forall i in 0..6
        var k = 0
        var odd = 0
        while k < i
            k = k + 1
            if (k - 1) % 2 == 0
                continue
            odd = odd + 1
        data[i] = odd
    let host = data
    println(f\"{host[0]} {host[1]} {host[2]} {host[3]} {host[4]} {host[5]}\")
",
        "0 0 1 1 2 2",
    );
}

/// A `while` loop mixing `continue` and `break`: iterations equal to `i` are
/// skipped and the loop leaves early once the running total passes 6.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn while_with_continue_and_break_in_a_forall_body() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io

fn main()
    gpu var data = [0, 0, 0, 0]
    forall i in 0..4
        var k = 0
        var total = 0
        while k < 5
            k = k + 1
            if k == i
                continue
            total = total + k
            if total > 6
                break
        data[i] = total
    let host = data
    println(f\"{host[0]} {host[1]} {host[2]} {host[3]}\")
",
        "10 9 8 7",
    );
}

/// The same loops on the CPU keep their meaning.
#[test]
fn while_and_forever_with_continue_on_the_cpu() {
    assert_runs_with_output(
        "
use system.io

fn main()
    var k = 0
    var odd = 0
    while k < 5
        k = k + 1
        if (k - 1) % 2 == 0
            continue
        odd = odd + 1
    var n = 0
    var total = 0
    forever
        n = n + 1
        if n > 5
            break
        if n == 2
            continue
        total = total + n
    println(f\"{odd} {total}\")
",
        "2 13",
    );
}

/// A `forever` loop whose body opens with its exit test: element `i` counts up
/// to `i`. The test at the top of the body is a branch inside the loop, not the
/// loop's own condition.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn forever_with_break_first_in_a_forall_body() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io

fn main()
    gpu var data = [0, 0, 0, 0]
    forall i in 0..4
        var k = 0
        forever
            if k == i: break
            k = k + 1
        data[i] = k
    let host = data
    println(f\"{host[0]} {host[1]} {host[2]} {host[3]}\")
",
        "0 1 2 3",
    );
}

/// A `forever` loop mixing `continue` and `break`: values equal to `i` are
/// skipped and the loop leaves once the counter passes 4.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn forever_with_continue_and_break_in_a_forall_body() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io

fn main()
    gpu var data = [0, 0, 0, 0]
    forall i in 0..4
        var n = 0
        var total = 0
        forever
            n = n + 1
            if n > 4
                break
            if n == i
                continue
            total = total + n
        data[i] = total
    let host = data
    println(f\"{host[0]} {host[1]} {host[2]} {host[3]}\")
",
        "10 9 8 7",
    );
}

/// An `until` loop in a kernel body runs while its condition is false.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn until_in_a_forall_body() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io

fn main()
    gpu var data = [0, 0, 0, 0]
    forall i in 0..4
        var k = 0
        until k == i
            k = k + 1
        data[i] = k * 10
    let host = data
    println(f\"{host[0]} {host[1]} {host[2]} {host[3]}\")
",
        "0 10 20 30",
    );
}

/// The same `until` and `forever` shapes on the CPU keep their meaning.
#[test]
fn until_and_forever_with_break_first_on_the_cpu() {
    assert_runs_with_output(
        "
use system.io

fn main()
    var k = 0
    until k == 3
        k = k + 1
    var n = 0
    forever
        if n == 4: break
        n = n + 1
    println(f\"{k} {n}\")
",
        "3 4",
    );
}

/// A `do … while` and a `do … until` in a kernel body run their body once
/// before the first test.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn do_while_and_do_until_in_a_forall_body() {
    require_gpu_int64();
    assert_runs_with_output(
        "
use system.io

fn main()
    gpu var data = [0, 0, 0, 0]
    forall i in 0..4
        var k = 0
        do
            k = k + 1
        while k < i
        var m = 0
        do
            m = m + 1
        until m >= i
        data[i] = k * 10 + m
    let host = data
    println(f\"{host[0]} {host[1]} {host[2]} {host[3]}\")
",
        "11 11 22 33",
    );
}
