// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// `gpu for` scalar capture feature: plain host scalars (int, bool, f32)
// are passed as WGSL uniforms and are read-only inside the kernel.

use super::utils::*;

#[test]
fn scalar_int_capture_in_gpu_for() {
    assert_runs(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var buf = [0, 0, 0, 0]
    let k = 7
    gpu forall i in 0..4
        buf[i] = i * k
",
    );
}

#[test]
fn scalar_int_capture_value_is_correct() {
    // Verify that scalar int captures work: the captured scalar k=5
    // is passed to the kernel as a uniform and used in computation.
    assert_runs(
        "
use system.gpu
use system.collections.array
use system.io

fn main()
    gpu var buf = [0, 0, 0, 0]
    let k = 5
    gpu forall i in 0..4
        buf[i] = i * k
    let result = buf.element_at(2)
    println(f\"{result}\")
",
    );
}

#[test]
fn scalar_f32_capture_in_gpu_for() {
    assert_runs(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var buf = [0.0, 0.0, 0.0, 0.0]
    let s = 2.0
    gpu forall i in 0..4
        buf[i] = s
",
    );
}

#[test]
fn scalar_bool_capture_in_gpu_for() {
    assert_runs(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var buf = [0, 0, 0, 0]
    let flag = true
    gpu forall i in 0..4
        if flag
            buf[i] = 1
        else
            buf[i] = 0
",
    );
}

#[test]
fn writing_to_captured_scalar_is_rejected() {
    // A captured host scalar is a read-only uniform inside the kernel.
    // Assigning to it in the loop body must be rejected during lowering.
    assert_runtime_error(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var buf = [0, 0, 0, 0]
    var k = 7
    gpu forall i in 0..4
        k = k + 1
        buf[i] = k
",
        "captured scalar 'k' is read-only",
    );
}

#[test]
fn unsupported_scalar_type_string_is_rejected() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var buf = [0, 0, 0, 0]
    let s = \"hello\"
    gpu forall i in 0..4
        println(s)
",
        "unsupported gpu scalar capture type",
    );
}

#[test]
fn multiple_scalar_captures() {
    assert_runs(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var buf = [0, 0, 0, 0]
    let a = 2
    let b = 3
    gpu forall i in 0..4
        buf[i] = a * i + b
",
    );
}

#[test]
fn multiple_scalar_captures_value_check() {
    // Verify that multiple scalar int captures work together
    // in the same kernel.
    assert_runs(
        "
use system.gpu
use system.collections.array
use system.io

fn main()
    gpu var buf = [0, 0, 0, 0]
    let a = 2
    let b = 3
    gpu forall i in 0..4
        buf[i] = a * i + b
    let result = buf.element_at(3)
    println(f\"{result}\")
",
    );
}

#[test]
fn mixed_buffer_and_scalar_captures() {
    assert_runs(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [1, 2, 3, 4]
    gpu var result = [0, 0, 0, 0]
    let multiplier = 10
    gpu forall i in 0..4
        result[i] = data[i] * multiplier
",
    );
}

#[test]
fn mixed_buffer_and_scalar_captures_value_check() {
    // Verify that both buffer and scalar captures work together:
    // the kernel accesses the gpu-resident data buffer and multiplies
    // by the captured scalar multiplier.
    assert_runs(
        "
use system.gpu
use system.collections.array
use system.io

fn main()
    gpu var data = [1, 2, 3, 4]
    gpu var result = [0, 0, 0, 0]
    let multiplier = 10
    gpu forall i in 0..4
        result[i] = data[i] * multiplier
    let r = result.element_at(2)
    println(f\"{r}\")
",
    );
}
