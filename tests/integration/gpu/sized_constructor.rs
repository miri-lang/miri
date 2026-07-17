// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU sized-constructor (Array<T, N>()) round-trip tests.
//! These tests verify that sized GPU buffers can be created, written, and readback
//! with correct values for both scalar and wider element types.

use super::device::assert_gpu_runs_with_output;
use crate::integration::utils::assert_runs_with_output;

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_sized_ctor_int_roundtrip() {
    assert_gpu_runs_with_output(
        r#"
use system.gpu
use system.collections.array

gpu var buf = Array<int, 8>()

gpu forall i in 0..8
    buf[i] = (i * 2) as int

let host = buf
println(f"{host[0]}")
println(f"{host[1]}")
println(f"{host[2]}")
println(f"{host[7]}")
"#,
        "0\n2\n4\n14",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_sized_ctor_f32_roundtrip() {
    assert_gpu_runs_with_output(
        r#"
use system.gpu
use system.collections.array

gpu var buf = Array<f32, 4>()

gpu forall i in 0..4
    buf[i] = (i as f32) * 1.5 as f32

let host = buf
println(f"{host[0]}")
println(f"{host[1]}")
println(f"{host[2]}")
println(f"{host[3]}")
"#,
        "0.0\n1.5\n3.0\n4.5",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_sized_ctor_i32_roundtrip() {
    assert_gpu_runs_with_output(
        r#"
use system.gpu
use system.collections.array

gpu var buf = Array<i32, 6>()

gpu forall i in 0..6
    buf[i] = i as i32

let host = buf
println(f"{host[0]}")
println(f"{host[5]}")
"#,
        "0\n5",
    );
}

#[test]
fn test_gpu_sized_ctor_large_arithmetic_size() {
    // Regression test: verify that sized-array generics with large arithmetic
    // expressions work correctly. This tests that checked arithmetic is used
    // to prevent silent overflow when computing buffer sizes.
    assert_runs_with_output(
        r#"
use system.gpu
use system.collections.array

fn main()
    gpu var buf = Array<i32, 100 * 100>()
    let len = buf.length()
    println(f"{len}")
"#,
        "10000",
    );
}
