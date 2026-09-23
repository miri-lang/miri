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
