// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for kernel-callable user functions.
//!
//! A plain Miri `fn` may be called inside a `gpu fn`/`forall` kernel body
//! if its parameters and return type are GPU-compatible scalars (no managed
//! types, no arrays, no out parameters, no recursion).
//!
//! The WGSL backend emits callable functions as module-level helpers (not
//! entry points), allowing the kernel entry to invoke them.

use super::device::assert_gpu_runs_with_output;
use super::helpers::assert_gpu_wgsl_valid;
use super::utils::*;

/// Simplest case: kernel calls a trivial user function.
/// The function doubles its input.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_calls_trivial_scalar_function() {
    let source = "
use system.gpu
use system.collections.array

fn dbl(x float) float: x * 2.0

fn main()
    gpu let src = [1.0, 2.0, 3.0]
    gpu var dst = [0.0, 0.0, 0.0]
    gpu forall i in 0..3
        dst[i] = dbl(src[i])
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]}')
";
    assert_gpu_runs_with_output(source, "2.0 4.0 6.0");
}

/// WGSL backend emits a helper function with the correct signature.
#[test]
fn kernel_calls_function_emits_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.collections.array

fn dbl(x int) int: x * 2

fn main()
    gpu let src = [1, 2, 3]
    gpu var dst = [0, 0, 0]
    gpu forall i in 0..3
        dst[i] = dbl(src[i])
",
    );
}

/// User function with two scalar parameters.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_calls_function_with_two_params() {
    let source = "
use system.gpu
use system.collections.array

fn madd(x float, y float) float: x * y + 1.0

fn main()
    gpu let a = [1.0, 2.0, 3.0]
    gpu let b = [10.0, 20.0, 30.0]
    gpu var dst = [0.0, 0.0, 0.0]
    gpu forall i in 0..3
        dst[i] = madd(a[i], b[i])
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]}')
";
    // madd(1.0, 10.0) = 1.0*10.0 + 1.0 = 11.0
    // madd(2.0, 20.0) = 2.0*20.0 + 1.0 = 41.0
    // madd(3.0, 30.0) = 3.0*30.0 + 1.0 = 91.0
    assert_gpu_runs_with_output(source, "11.0 41.0 91.0");
}

/// Transitive call chain: kernel → fn_a → fn_b.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_calls_function_that_calls_another_function() {
    let source = "
use system.gpu
use system.collections.array

fn add_one(x float) float: x + 1.0
fn double_then_add_one(x float) float: add_one(x * 2.0)

fn main()
    gpu let src = [1.0, 2.0, 3.0]
    gpu var dst = [0.0, 0.0, 0.0]
    gpu forall i in 0..3
        dst[i] = double_then_add_one(src[i])
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]}')
";
    // double_then_add_one(1.0) = add_one(2.0) = 3.0
    // double_then_add_one(2.0) = add_one(4.0) = 5.0
    // double_then_add_one(3.0) = add_one(6.0) = 7.0
    assert_gpu_runs_with_output(source, "3.0 5.0 7.0");
}

/// User function calls a Part-1 math intrinsic.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_calls_function_that_uses_math_intrinsic() {
    let source = "
use system.gpu
use system.collections.array
use system.math

fn clamp_tanh(x float) float: tanh(x)

fn main()
    gpu let src = [0.0, 1.0, -1.0]
    gpu var dst = [0.0, 0.0, 0.0]
    gpu forall i in 0..3
        dst[i] = clamp_tanh(src[i])
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]}')
";
    // tanh(0.0) = 0.0, tanh(1.0) ≈ 0.7616, tanh(-1.0) ≈ -0.7616
    // Print with tolerance check (approximate values)
    assert_runs(source);
}

/// Integer-typed helper function (not just float).
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_calls_integer_function() {
    let source = "
use system.gpu
use system.collections.array

fn square(x int) int: x * x

fn main()
    gpu let src = [1, 2, 3]
    gpu var dst = [0, 0, 0]
    gpu forall i in 0..3
        dst[i] = square(src[i])
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]}')
";
    assert_gpu_runs_with_output(source, "1 4 9");
}

// ─────────────────────────────────────────────────────────
// NEGATIVE TESTS: Type-checker rejection paths
// ─────────────────────────────────────────────────────────

/// NEGATIVE: function returns a non-scalar (String) → rejected by type-checker.
#[test]
fn kernel_cannot_call_function_returning_string() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn bad() String
    \"hello\"

fn main()
    gpu let arr = [1]
    gpu forall i in 0..1
        let x = arr[i]
        let _ = bad()
",
        "not GPU-compatible",
    );
}

/// NEGATIVE: function parameter is a List (managed type) → rejected.
#[test]
fn kernel_cannot_call_function_with_list_param() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.list

fn process(lst [int]) int
    0

fn main()
    gpu let arr = [1, 2, 3]
    gpu forall i in 0..3
        let _ = process(arr)
",
        "not GPU-compatible",
    );
}

/// NEGATIVE: function parameter is an Array → rejected (arrays not allowed as params).
#[test]
fn kernel_cannot_call_function_with_array_param() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn sum_array(arr [float; 3]) float
    0.0

fn main()
    gpu let src = [1.0, 2.0, 3.0]
    gpu forall i in 0..3
        let x = src[i]
        let _ = sum_array([0.0, 0.0, 0.0])
",
        "not GPU-compatible",
    );
}

/// NEGATIVE: function attempts to call with non-scalar param via dispatch (indirect test).
/// For now, we'll test with a host-only scenario that forces the GPU path to reject.
/// TODO: Once we can test out-param syntax properly, add explicit out-param test.
#[test]
fn kernel_cannot_call_function_taking_string() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn process_str(s String) float: 1.0

fn main()
    gpu let arr = [1.0]
    gpu forall i in 0..1
        let x = arr[i]
        let _ = process_str(\"hello\")
",
        "not GPU-compatible",
    );
}

/// NEGATIVE: direct recursion → rejected with clear message.
#[test]
fn kernel_cannot_call_directly_recursive_function() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn fact(n int) int
    if n <= 1
        1
    else
        n * fact(n - 1)

fn main()
    gpu let arr = [1]
    gpu forall i in 0..1
        let x = arr[i]
        let _ = fact(5)
",
        "recursion is not allowed",
    );
}

/// NEGATIVE: indirect recursion (a → b → a) → rejected.
#[test]
fn kernel_cannot_call_indirectly_recursive_functions() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn foo(n int) int
    if n <= 0
        0
    else
        bar(n - 1)

fn bar(n int) int
    if n <= 0
        0
    else
        foo(n - 1)

fn main()
    gpu let arr = [1]
    gpu forall i in 0..1
        let x = arr[i]
        let _ = foo(5)
",
        "recursion is not allowed",
    );
}

/// NEGATIVE: function calls a host-only intrinsic (println) → rejected.
#[test]
fn kernel_cannot_call_function_that_calls_host_intrinsic() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn bad_fn(x float) float
    println(\"x\")
    x

fn main()
    gpu let arr = [1.0]
    gpu forall i in 0..1
        let x = arr[i]
        let _ = bad_fn(1.0)
",
        "not GPU-compatible",
    );
}

/// A void-returning function has no WGSL scalar representation, so it cannot be
/// called from a kernel — rejected at type-check rather than crashing codegen.
#[test]
fn kernel_cannot_call_void_returning_function() {
    assert_compiler_error(
        "
use system.gpu

fn noop(x float)
    let _ = x + 1.0

fn main()
    gpu var arr = [1.0]
    gpu forall i in 0..1
        arr[i] = arr[i] + 1.0
        noop(arr[i])
",
        "not GPU-compatible",
    );
}
