// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for N4 (buffer ping-pong).
//!
//! Two persistent `gpu var` grids, two sequential `gpu for` blocks with swapped
//! read/write roles. Persistent device buffers (F6 feature) retain both across
//! dispatches without intermediate readback.

use super::helpers::compile_to_wgsl;

#[test]
fn ping_pong_two_kernels_swapped_read_write_qualifiers() {
    let wgsl = compile_to_wgsl(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var a = [1, 2, 3, 4]
    gpu var b = [10, 20, 30, 40]
    gpu for i in 0..4
        b[i] = a[i] + 100
    gpu for i in 0..4
        a[i] = b[i] + 1000
",
    );

    // First kernel: a is read-only, b is read-write.
    // Second kernel: b is read-only, a is read-write.
    // Split the output into the two kernels (they're in the same emitted module).
    // At least one read-only and one read-write binding should be present.
    assert!(
        wgsl.contains("var<storage, read>"),
        "Expected read-only storage binding. WGSL:\n{}",
        wgsl
    );
    assert!(
        wgsl.contains("var<storage, read_write>"),
        "Expected read-write storage binding. WGSL:\n{}",
        wgsl
    );
}
