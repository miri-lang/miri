// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// `forall` scalar capture feature: plain host scalars (int, bool, f32)
// are passed as WGSL uniforms and are read-only inside the kernel.

use super::utils::*;

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn scalar_int_capture_value_is_correct() {
    // Verify that scalar int captures work: the captured scalar k=5
    // is passed to the kernel as a uniform and used in computation.
    // buf[2] = 2 * 5 = 10 after the kernel runs. Reading the value from
    // host requires an explicit readback ('let host = buf').
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var buf = [0, 0, 0, 0]
    let k = 5
    gpu forall i in 0..4
        buf[i] = i * k
    let host = buf
    let result = host.element_at(2)
    println(f\"{result}\")
",
        "10",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
    // A captured outer scalar written inside a forall body is a loop-carried
    // accumulator, which is rejected at type-check because forall requires order-independent iterations.
    assert_compiler_error(
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
        "loop-carried accumulator 'k'",
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn multiple_scalar_captures_value_check() {
    // Verify that multiple scalar int captures work together
    // in the same kernel. buf[3] = 2 * 3 + 3 = 9 after the kernel runs.
    // Reading the value from host requires an explicit readback.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var buf = [0, 0, 0, 0]
    let a = 2
    let b = 3
    gpu forall i in 0..4
        buf[i] = a * i + b
    let host = buf
    let result = host.element_at(3)
    println(f\"{result}\")
",
        "9",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn mixed_buffer_and_scalar_captures_value_check() {
    // Verify that both buffer and scalar captures work together:
    // the kernel accesses the gpu-resident data buffer and multiplies
    // by the captured scalar multiplier. result[2] = 3 * 10 = 30.
    // Reading the value from host requires an explicit readback.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [1, 2, 3, 4]
    gpu var result = [0, 0, 0, 0]
    let multiplier = 10
    gpu forall i in 0..4
        result[i] = data[i] * multiplier
    let host = result
    let r = host.element_at(2)
    println(f\"{r}\")
",
        "30",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn module_const_in_forall_body_computes_on_device() {
    // A module-level `const` referenced inside a `gpu forall` body must carry its
    // real value onto the device (inlined as a literal), not silently read 0.
    // buf[3] = 64 + 3 = 67 proves the constant reached the kernel; a dropped
    // capture would have yielded 3.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

const W = 64

fn main()
    gpu var buf = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i in 0..8
        buf[i] = W + i
    let host = buf
    println(f\"{host.element_at(3)}\")
",
        "67",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn arithmetic_module_const_in_forall_body_computes_on_device() {
    // An arithmetic module const (`64 * 64`) must const-fold and reach the kernel
    // body. buf[5] = 4096 + 5 = 4101 proves the folded value 4096 is present.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

const SIZE = 64 * 64

fn main()
    gpu var buf = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i in 0..8
        buf[i] = SIZE + i
    let host = buf
    println(f\"{host.element_at(5)}\")
",
        "4101",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn local_const_in_forall_body_computes_on_device() {
    // A function-local `const` captured into a `gpu forall` body is uploaded as a
    // uniform. buf[3] = 64 + 3 = 67 proves the uniform carried 64, not 0.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

fn main()
    const W = 64
    gpu var buf = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i in 0..8
        buf[i] = W + i
    let host = buf
    println(f\"{host.element_at(3)}\")
",
        "67",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn module_const_in_bare_forall_body_computes_on_device() {
    // A bare `forall` over a gpu-resident buffer routes to GPU; a module const in
    // its body must still carry its value. buf[3] = 64 + 3 = 67.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

const W = 64

fn main()
    gpu var buf = [0, 0, 0, 0, 0, 0, 0, 0]
    forall i in 0..8
        buf[i] = W + i
    let host = buf
    println(f\"{host.element_at(3)}\")
",
        "67",
    );
}

/// A named `const` forall bound and a captured host scalar coexist: the const
/// bound lowers to a runtime loop-bound uniform, and the scalar `2.5` must reach
/// the kernel as its own uniform rather than reading the bound's value. Both
/// buffer slots must read back `2.5`. Regresses the binding-order clash where the
/// scalar `_Inputs` uniform and the loop-bound uniform were assigned mismatched
/// binding indices between the WGSL emitter and the runtime host driver.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn scalar_float_capture_with_const_bound_value_check() {
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

const N = 2

fn main()
    gpu var dst = [0.0, 0.0]
    let a = 2.5
    gpu forall i in 0..N
        dst[i] = a
    let host = dst
    println(f\"{host.element_at(0)} {host.element_at(1)}\")
",
        "2.5 2.5",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn scalar_capture_with_variable_bound_value_check() {
    // A runtime (variable) forall bound and a captured scalar share the kernel:
    // the bound `n` lowers to a loop-bound uniform, the scalar `k` to the pooled
    // scalar `_Inputs` uniform. buf[3] = 3 * 4 = 12 proves the scalar carried 4,
    // not the bound value. Guards the WGSL/runtime binding-order agreement for a
    // genuinely runtime bound, not only a const-folded one.
    assert_runs_with_output(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var buf = [0, 0, 0, 0]
    let n = 4
    let k = 4
    gpu forall i in 0..n
        buf[i] = i * k
    let host = buf
    println(f\"{host.element_at(3)}\")
",
        "12",
    );
}
