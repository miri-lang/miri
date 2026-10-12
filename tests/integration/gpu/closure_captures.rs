// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A `forall` inside a closure launches over the closure's captures, so every
//! buffer the kernel writes has to be captured even when the closure body
//! mentions it nowhere else.
//!
//! A capture is a copy, on the device as on the host: the kernel writes the
//! closure's buffer, and the enclosing binding keeps the values it had.

use super::device::require_gpu_int64;
use super::utils::*;

const FORALL_IN_CLOSURE: &str = r#"

fn main()
    gpu var result = [0, 0, 0]
    let f = fn()
        forall i in 0..3
            result[i] = i * 2
        let inside = result
        println(f"{inside[0]} {inside[1]} {inside[2]}")
        return
    f()
    let h = result
    println(f"{h[0]} {h[1]} {h[2]}")
"#;

#[test]
fn forall_in_closure_captures_the_buffer_it_writes() {
    assert_builds(FORALL_IN_CLOSURE);
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn forall_in_closure_writes_the_captured_buffer() {
    require_gpu_int64();
    assert_runs_with_output(FORALL_IN_CLOSURE, "0 2 4\n0 0 0");
}
