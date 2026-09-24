// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Negative tests for GPU lowering error paths: reduce fold arity and
// gpu fn buffer-argument shape.

use super::utils::assert_build_error;

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
    let sum = a.reduce(0, fn(x i32) i32: x)
",
        "expected Function(int, int) -> int, got Function(i32) -> i32",
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
    let sum = a.reduce(0, fn(acc i32, x i32, y i32) i32: acc + x)
",
        "expected Function(int, int) -> int, got Function(i32, i32, i32) -> i32",
    );
}

/// A gpu fn buffer argument written as an inline expression is rejected:
/// the temporary it materializes is host-resident, and only gpu-resident
/// buffers may be bound to a kernel launch.
#[test]
fn gpu_fn_buffer_arg_expression_is_rejected() {
    assert_build_error(
        "
use system.collections.array

gpu fn my_kernel(a Array<f32,4>)
    let x = 1

fn main()
    my_kernel([1.0, 2.0, 3.0, 4.0]).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
",
        "cannot pass host-resident array",
    );
}

/// A GPU buffer whose const size expression underflows (`A - B` with `A < B`)
/// is rejected rather than silently folding to a zero-length buffer. The size
/// folder uses checked subtraction, so the underflow leaves the size
/// unresolved and the buffer fails to type-check.
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
        "non-negative",
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
    let s = data.reduce(0, fn(a i32, b i32) i32: a + a)
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
    let s = data.reduce(1, fn(a i32, b i32) i32: b * b)
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
    let s = data.reduce(0, fn(a i32, b i32) i32: b + a)
",
    );
}
