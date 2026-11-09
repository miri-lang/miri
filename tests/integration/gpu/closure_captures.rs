// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A `forall` inside a closure writes a buffer the closure declares itself.
//! A capture is a copy, on the device as on the host, so a kernel may not
//! write a captured buffer: the write would be lost when the call ends.

use super::device::require_gpu_int64;
use super::utils::*;

const FORALL_IN_CLOSURE: &str = r#"

fn main()
    gpu var result = [1, 2, 3]
    let f = fn()
        gpu var mine = [0, 0, 0]
        forall i in 0..3
            mine[i] = i * 2
        let inside = mine
        println(f"{inside[0]} {inside[1]} {inside[2]}")
        return
    f()
    let h = result
    println(f"{h[0]} {h[1]} {h[2]}")
"#;

#[test]
fn forall_in_closure_writes_the_buffer_the_closure_declares() {
    assert_builds(FORALL_IN_CLOSURE);
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn forall_in_closure_writes_its_own_buffer_on_the_device() {
    require_gpu_int64();
    assert_runs_with_output(FORALL_IN_CLOSURE, "0 2 4\n1 2 3");
}

#[test]
fn forall_in_closure_writing_a_captured_buffer_is_refused() {
    assert_compiler_error(
        r#"

fn main()
    gpu var result = [0, 0, 0]
    let f = fn()
        forall i in 0..3
            result[i] = i * 2
        return
    f()
"#,
        "MER_TYP_090",
    );
}
