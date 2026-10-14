// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Negative tests for GPU lowering error paths: reduce fold arity and
// gpu fn buffer-argument shape.

use super::utils::{assert_build_error, assert_compiler_error, assert_runs_with_output};

/// A reduce fold function with one parameter is rejected at type checking:
/// the fold combines an accumulator and an element, so the expected callback
/// signature is a two-parameter function.
#[test]
fn reduce_fold_with_one_param_is_rejected() {
    assert_build_error(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    let sum = a.reduce(0, fn(x int) int: x)
",
        "expected Function(int, int) -> int, got Function(int) -> int",
    );
}

/// A reduce fold function with three parameters is rejected for the same
/// two-parameter callback contract.
#[test]
fn reduce_fold_with_three_params_is_rejected() {
    assert_build_error(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    let sum = a.reduce(0, fn(acc int, x int, y int) int: acc + x)
",
        "expected Function(int, int) -> int, got Function(int, int, int) -> int",
    );
}

/// A GPU buffer whose const size expression folds below zero (`A - B` with
/// `A < B`) is rejected rather than silently folding to a zero-length buffer:
/// a value argument must be greater than zero.
#[test]
fn gpu_buffer_const_size_underflow_is_rejected() {
    assert_build_error(
        "
use system.gpu

const A = 2
const B = 5

fn main()
    gpu var buf = Array<f32, A - B>()
",
        "MER_TYP_077",
    );
}

/// A function-scope scratch array sized by a runtime value (non-constant `N`)
/// is a compile-time error, not an internal codegen panic. WGSL function
/// arrays must be fixed-size, so the size must const-evaluate.
#[test]
fn local_array_with_runtime_size_is_rejected() {
    assert_build_error(
        "
use system.gpu
use system.collections.array

fn main()
    var n = 4
    gpu var out = Array<f32, 4>()
    gpu forall t in 0..1
        var h = Array<f32, n>()
        h[0] = 1.0
        out[0] = h[0] as f32
",
        "requires a compile-time constant size",
    );
}

/// A fold that names one parameter twice (`a + a`) is not the parameter pair
/// the tree reduction combines; it is refused instead of reduced as `a + b`.
#[test]
fn reduce_fold_repeating_its_first_param_is_rejected() {
    assert_build_error(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [1, 2, 3, 4]
    let s = data.reduce(0, fn(a int, b int) int: a + a)
",
        "reduce fold operands must be the two fold parameters",
    );
}

/// The same holds for the second parameter under `*`.
#[test]
fn reduce_fold_repeating_its_second_param_is_rejected() {
    assert_build_error(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [1, 2, 3, 4]
    let s = data.reduce(1, fn(a int, b int) int: b * b)
",
        "reduce fold operands must be the two fold parameters",
    );
}

/// Both parameters in either order are the supported fold.
#[test]
fn reduce_fold_with_swapped_params_is_accepted() {
    super::utils::assert_builds(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [1, 2, 3, 4]
    let s = data.reduce(0, fn(a int, b int) int: b + a)
",
    );
}

/// A kernel-body binding initialized with a whole buffer would copy device
/// storage into a local, which a kernel cannot do (the shader is invalid); it is
/// refused where it is written, with what to write instead.
#[test]
fn copying_a_whole_buffer_inside_a_forall_body_is_refused() {
    assert_compiler_error(
        "
use system.io

fn main()
    gpu var g = [1, 2, 3, 4]
    forall i in 0..4
        var a = g
        a[i] = a[i] + 1
    let host = g
    println(f\"{host[0]}\")
",
        "a kernel cannot copy the buffer 'g' into 'a'",
    );
}

/// The same copy of a `gpu fn` buffer parameter is refused under the same code.
#[test]
fn copying_a_buffer_parameter_inside_a_gpu_fn_is_refused() {
    assert_compiler_error(
        "
use system.collections.array

gpu fn k(data out Array<int, 4>)
    let copy = data
    data[0] = copy[1]

fn main()
    gpu var buf = [1, 2, 3, 4]
    k(buf).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
",
        "error[MER_TAR_006]",
    );
}

/// Copying one element of the buffer into a kernel local is what a kernel
/// does, and runs.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn copying_an_element_of_a_buffer_inside_a_forall_body_runs() {
    super::device::require_gpu_int64();
    assert_runs_with_output(
        "
use system.io

fn main()
    gpu var g = [1, 2, 3, 4]
    forall i in 0..4
        var a = g[i]
        g[i] = a * 10
    let host = g
    println(f\"{host[0]} {host[3]}\")
",
        "10 40",
    );
}
