// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for kernel-callable user functions.
//!
//! A plain Miri `fn` may be called inside a `gpu fn`/`forall` kernel body
//! if its parameters, `out` ones included, and its return type, if it has
//! one, are GPU-compatible scalars (no managed types, no arrays, no
//! recursion).
//!
//! The WGSL backend emits callable functions as module-level helpers (not
//! entry points), allowing the kernel entry to invoke them. An `out`
//! parameter is a `ptr<function, T>` the helper reads and writes as `(*_k)`;
//! the call copies the place it names (a whole local, a buffer element, a
//! vector component) into a function variable, hands over `&_t` and stores
//! the temp back after the call, so no pointer ever reaches storage or a
//! sub-object. A helper returning nothing declares no result and is called
//! as a statement. The same helper gives the same values on the host.

use super::device::assert_gpu_runs_with_output;
use super::helpers::{
    assert_gpu_wgsl_valid, assert_gpu_wgsl_valid_in, compile_kernel_to_hlsl,
    compile_kernel_to_spirv, compile_to_wgsl,
};
use super::utils::*;
use crate::utils::miri_run;

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
        dst[i] = dbl(src[i]) as f32
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
        dst[i] = madd(a[i], b[i]) as f32
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
        dst[i] = double_then_add_one(src[i]) as f32
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]}')
";
    // double_then_add_one(1.0) = add_one(2.0) = 3.0
    // double_then_add_one(2.0) = add_one(4.0) = 5.0
    // double_then_add_one(3.0) = add_one(6.0) = 7.0
    assert_gpu_runs_with_output(source, "3.0 5.0 7.0");
}

/// A helper calling another helper is declared, and called, under its WGSL
/// name in the kernel module: the chain validates without a device.
#[test]
fn kernel_calls_function_that_calls_another_function_emits_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.collections.array

fn add_one(x float) float: x + 1.0
fn double_then_add_one(x float) float: add_one(x * 2.0)

fn main()
    gpu let src = [1.0, 2.0, 3.0]
    gpu var dst = [0.0, 0.0, 0.0]
    gpu forall i in 0..3
        dst[i] = double_then_add_one(src[i]) as f32
",
    );
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
        dst[i] = clamp_tanh(src[i]) as f32
    let host = dst
    println(f'{host[0]}')
";
    // tanh(0.0) = 0.0 exactly, deterministic value to assert
    assert_gpu_runs_with_output(source, "0.0");
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

/// A generic helper is instantiated at the type each kernel call passes, and
/// that instantiation is declared in the kernel's module like any helper.
#[test]
fn kernel_calls_a_generic_function() {
    assert_gpu_runs_with_output(
        r#"
use system.io
use system.gpu
use system.collections.array

fn dbl<T>(x T) T: x + x

fn main()
    gpu let src = [1, 2, 3]
    gpu var dst = [0, 0, 0]
    gpu forall i in 0..3
        dst[i] = dbl(src[i])
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]}')
"#,
        "2 4 6",
    );
}

/// Two instantiations of one generic helper reached from one kernel are two
/// helpers, each at its own type.
#[test]
fn kernel_calls_a_generic_function_at_two_types_emits_valid_wgsl() {
    assert_gpu_wgsl_valid(
        r#"
use system.gpu
use system.collections.array

fn dbl<T>(x T) T: x + x

fn main()
    gpu var ints = [1, 2, 3]
    gpu var floats [f32; 3] = [0.5, 1.5, 2.5]
    gpu forall i in 0..3
        ints[i] = dbl(ints[i])
        floats[i] = dbl(floats[i])
"#,
    );
}

/// A generic helper instantiated at a type GPU code cannot hold is refused
/// naming that type, not the parameter it was written as.
#[test]
fn kernel_call_to_a_generic_function_at_a_host_type_is_refused() {
    assert_compiler_error(
        r#"
use system.gpu
use system.collections.array

fn first<T>(x T, _y int) T: x

fn main()
    gpu var dst = [0, 0, 0]
    gpu forall i in 0..3
        let s = first("a", i)
        dst[i] = i
"#,
        "Type 'String' is not GPU-compatible",
    );
}

/// A static method is a helper like a free function: its WGSL signature drops
/// the allocator every Miri function carries on the CPU, as its call does.
#[test]
fn kernel_calls_static_method_emits_valid_wgsl() {
    assert_gpu_wgsl_valid(
        r#"
use system.collections.array

class P
    static fn norm(x int) int
        return x * 3

fn main()
    gpu var a = [1, 2, 3, 4]
    forall i in 0..a.length()
        a[i] = P.norm(a[i])
"#,
    );
}

/// The static method's result reaches every element.
#[test]
fn kernel_calls_static_method() {
    assert_gpu_runs_with_output(
        r#"
use system.collections.array

class P
    static fn norm(x int) int
        return x * 3

fn main()
    gpu var a = [1, 2, 3, 4]
    forall i in 0..a.length()
        a[i] = P.norm(a[i])
    let h = a
    println(f"{h[0]} {h[3]}")
"#,
        "3 12",
    );
}

/// A release build names no locals, so the allocator is told apart by where
/// the body keeps it, not by its name: a helper calling a helper stays valid.
#[test]
fn kernel_helpers_emit_valid_wgsl_in_a_release_build() {
    assert_gpu_wgsl_valid_in(
        r#"
use system.collections.array

class P
    static fn norm(x int) int
        return x * 3

fn add_one(x int) int: x + 1
fn norm_then_add_one(x int) int: add_one(P.norm(x))

fn main()
    gpu var a = [1, 2, 3, 4]
    forall i in 0..a.length()
        a[i] = norm_then_add_one(a[i])
"#,
        true,
    );
}

/// `A.b_c` and `A_b.c` differ only in where the underscore falls between
/// owner and method; both are called from one kernel and each gives its own
/// result.
#[test]
fn kernel_calls_methods_differing_only_in_underscore_placement() {
    assert_gpu_runs_with_output(
        r#"
use system.collections.array

class A
    static fn b_c(x int) int
        return x + 1

class A_b
    static fn c(x int) int
        return x + 100

fn main()
    gpu var a = [1, 2, 3, 4]
    forall i in 0..a.length()
        a[i] = A.b_c(a[i]) + A_b.c(a[i])
    let h = a
    println(f"{h[0]} {h[3]}")
"#,
        "103 109",
    );
}

/// WGSL reserves identifiers beginning `__`; a method of a class so named is
/// still declared under a name the module may use.
#[test]
fn kernel_calls_method_of_a_class_named_with_the_reserved_prefix_emits_valid_wgsl() {
    assert_gpu_wgsl_valid(
        r#"
use system.collections.array

class __X
    static fn m(x int) int
        return x * 2

fn main()
    gpu var a = [1, 2, 3, 4]
    forall i in 0..a.length()
        a[i] = __X.m(a[i])
"#,
    );
}

/// NEGATIVE: a host function keeping a resource parameter, called from a
/// `gpu forall` body, is refused where the kernel calls it as a type error:
/// a resource is no GPU type, so the call never reaches lowering, which
/// binds a parameter the function keeps on the host only.
#[test]
fn kernel_cannot_call_function_keeping_a_resource_param() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

class Handle
    var id int

    fn drop(self)
        println(f'released {self.id}')

fn keep(h Handle) int
    var held = h
    held.id

fn main()
    gpu var dst = [0, 0, 0]
    gpu forall i in 0..3
        dst[i] = keep(Handle(i))
",
        "Function 'keep' parameter 'h' has type 'Handle' which is not GPU-compatible",
    );
}

/// The helpers the `out` tests call: one `bump` for every target.
const OUT_HELPERS: &str = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn bump(x out int)
    x = x + 1

fn bumpf(x out f32)
    x = x + 1.0
";

/// A kernel module calling `bump` on a whole local: the helper takes a
/// pointer to a function variable and the call hands it the temp's address.
const BUMP_A_LOCAL: &str = "
fn main()
    gpu var dst = [0, 1, 2, 3]
    gpu forall i in 0..4
        var k = dst[i]
        bump(k)
        dst[i] = k
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";

/// An `out` parameter is a pointer to a function variable, read and written
/// through `(*p)`, and the call hands over a temp's address: a void helper
/// declares no result.
#[test]
fn kernel_out_helper_takes_a_function_pointer() {
    let wgsl = compile_to_wgsl(&format!("{OUT_HELPERS}{BUMP_A_LOCAL}"));
    assert!(wgsl.contains(": ptr<function, i32>) {"), "{wgsl}");
    assert!(wgsl.contains("(*_1)"), "{wgsl}");
    assert!(wgsl.contains("(&_"), "{wgsl}");
    assert!(!wgsl.contains("ptr<storage"), "{wgsl}");
}

#[test]
fn kernel_out_helper_emits_valid_wgsl() {
    assert_gpu_wgsl_valid(&format!("{OUT_HELPERS}{BUMP_A_LOCAL}"));
}

/// The pointer parameter cross-compiles to the Vulkan and D3D shader forms.
#[test]
fn kernel_out_helper_cross_compiles_to_spirv_and_hlsl() {
    let source = format!("{OUT_HELPERS}{BUMP_A_LOCAL}");
    assert!(!compile_kernel_to_spirv(&source).is_empty());
    assert!(!compile_kernel_to_hlsl(&source).is_empty());
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_out_on_a_whole_local() {
    assert_gpu_runs_with_output(&format!("{OUT_HELPERS}{BUMP_A_LOCAL}"), "1 2 3 4");
}

/// A buffer element is copied into a temp, the temp handed over and stored
/// back into the element.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_out_on_a_buffer_element() {
    assert_gpu_runs_with_output(
        &format!(
            "{OUT_HELPERS}
fn main()
    gpu var dst = [10, 20, 30, 40]
    gpu forall i in 0..4
        bump(dst[i])
    let host = dst
    println(f'{{host[0]}} {{host[1]}} {{host[2]}} {{host[3]}}')
"
        ),
        "11 21 31 41",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_out_on_a_component_of_a_local_vector() {
    assert_gpu_runs_with_output(
        &format!(
            "{OUT_HELPERS}
fn main()
    gpu var dst [f32; 4] = [1.0, 2.0, 3.0, 4.0]
    gpu forall i in 0..4
        var v = Vec2<f32>(dst[i], 0.0)
        bumpf(v.x)
        dst[i] = v.x + v.y
    let host = dst
    println(f'{{host[0]}} {{host[1]}} {{host[2]}} {{host[3]}}')
"
        ),
        "2.0 3.0 4.0 5.0",
    );
}

/// Only the named component changes; its sibling keeps its value.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_out_on_a_component_of_a_buffer_vector() {
    assert_gpu_runs_with_output(
        &format!(
            "{OUT_HELPERS}
fn main()
    gpu var pts = [Vec2<f32>(1.0, 2.0), Vec2<f32>(3.0, 4.0)]
    gpu forall i in 0..2
        bumpf(pts[i].x)
    let host = pts
    println(f'{{host[0].x}} {{host[0].y}} {{host[1].x}} {{host[1].y}}')
"
        ),
        "2.0 2.0 4.0 4.0",
    );
}

/// A helper returning nothing hands its result back only through `out`.
const VOID_HELPER: &str = "
use system.gpu
use system.collections.array

fn twice(x int, r out int)
    r = x * 2

fn main()
    gpu let src = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        twice(src[i], dst[i])
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";

#[test]
fn kernel_void_helper_emits_valid_wgsl() {
    assert_gpu_wgsl_valid(VOID_HELPER);
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_void_helper_writes_through_out() {
    assert_gpu_runs_with_output(VOID_HELPER, "2 4 6 8");
}

/// A helper leaving early still has its writes stored back: the lanes that
/// return keep the first increment, the others take both.
const EARLY_RETURN_HELPER: &str = "
use system.gpu
use system.collections.array

fn grow(x out int)
    x = x + 1
    if x > 25
        return
    x = x + 100

fn main()
    gpu var dst = [10, 20, 30, 40]
    gpu forall i in 0..4
        grow(dst[i])
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";

#[test]
fn kernel_out_helper_with_an_early_return_emits_valid_wgsl() {
    assert_gpu_wgsl_valid(EARLY_RETURN_HELPER);
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_out_helper_with_an_early_return_stores_back() {
    assert_gpu_runs_with_output(EARLY_RETURN_HELPER, "111 121 31 41");
}

/// One `bump` runs on the host and on the device, and gives each the same
/// elements.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_out_helper_matches_the_host() {
    assert_gpu_runs_with_output(
        &format!(
            "{OUT_HELPERS}
fn main()
    gpu var dst = [10, 20, 30, 40]
    gpu forall i in 0..4
        bump(dst[i])
    var cpu = [10, 20, 30, 40]
    for j in 0..4
        bump(cpu[j])
    let host = dst
    println(f'{{host[0]}} {{host[1]}} {{host[2]}} {{host[3]}}')
    println(f'{{cpu[0]}} {{cpu[1]}} {{cpu[2]}} {{cpu[3]}}')
"
        ),
        "11 21 31 41\n11 21 31 41",
    );
}

/// A `bool` and a `u32` local each pass through `out` at their own width:
/// the flag flips on every lane and the counter steps from 7 to 8.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_out_on_a_bool_and_a_u32_local() {
    assert_gpu_runs_with_output(
        "
use system.gpu
use system.collections.array

fn flip(b out bool)
    b = not b

fn step(x out u32)
    x = x + 1

fn main()
    gpu var dst = [0, 1, 2, 3]
    gpu forall i in 0..4
        var b = i > 1
        flip(b)
        var u u32 = 7
        step(u)
        if b
            dst[i] = 100
        else
            dst[i] = 200
        if u == 8
            dst[i] = dst[i] + 1
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
",
        "101 101 201 201",
    );
}

/// One call takes two elements of two buffers, and stores each back.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_out_on_two_elements_in_one_call() {
    assert_gpu_runs_with_output(
        "
use system.gpu
use system.collections.array

fn two(a out int, b out int)
    a = a + 1
    b = b + 10

fn main()
    gpu var dst = [0, 1, 2, 3]
    gpu var aux = [100, 200, 300, 400]
    gpu forall i in 0..4
        two(dst[i], aux[i])
    let host = dst
    let other = aux
    println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
    println(f'{other[0]} {other[1]} {other[2]} {other[3]}')
",
        "1 2 3 4\n110 210 310 410",
    );
}

/// A helper hands its own `out` parameter on to another helper, twice when
/// the first step leaves it above 2.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_out_parameter_handed_on_by_a_nested_helper() {
    assert_gpu_runs_with_output(
        "
use system.gpu
use system.collections.array

fn bump(x out int)
    x = x + 1

fn outer(x out int)
    bump(x)
    if x > 2
        bump(x)

fn main()
    gpu var dst = [0, 1, 2, 3]
    gpu forall i in 0..4
        outer(dst[i])
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
",
        "1 2 4 5",
    );
}

/// An `out` call under an `if` and in a `while` of the kernel, on the lane's
/// own element: lane `i` takes one step past 1 and `i` steps in the loop.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_out_call_under_if_and_while() {
    assert_gpu_runs_with_output(
        &format!(
            "{OUT_HELPERS}
fn main()
    gpu var dst = [0, 1, 2, 3]
    gpu forall i in 0..4
        if i > 1
            bump(dst[i])
        var n = 0
        while n < i
            bump(dst[i])
            n = n + 1
    let host = dst
    println(f'{{host[0]}} {{host[1]}} {{host[2]}} {{host[3]}}')
"
        ),
        "0 2 5 7",
    );
}

/// Sub-word buffer elements pass through `out` at their declared widths,
/// and an element no lane hands over keeps its value.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_out_on_u8_and_i16_elements() {
    assert_gpu_runs_with_output(
        "
use system.gpu
use system.collections.array

fn bump8(x out u8)
    x = x + 1

fn bump16(x out i16)
    x = x + 1

fn main()
    gpu var a [u8; 4] = [1, 2, 3, 255]
    gpu var b [i16; 4] = [1, 2, 3, 4]
    gpu forall i in 0..4
        if i < 3
            bump8(a[i])
        bump16(b[i])
    let h = a
    let g = b
    println(f'{h[0]} {h[1]} {h[2]} {h[3]}')
    println(f'{g[0]} {g[1]} {g[2]} {g[3]}')
",
        "2 3 4 255\n2 3 4 5",
    );
}

/// A helper overflowing the `u8` it was handed through `out` stops the
/// program with the host's overflow trap.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_out_helper_overflow_stops_the_program() {
    assert_runtime_error(
        "
use system.gpu
use system.collections.array

fn bump8(x out u8)
    x = x + 1

fn main()
    gpu var a [u8; 4] = [1, 2, 3, 255]
    gpu forall i in 0..4
        bump8(a[i])
    let h = a
    println(f'{h[0]} {h[3]}')
",
        "MER_RT_003",
    );
}

/// The same value-sensitive helper on the device and on the host, each
/// handed one element as both its lent and its `out` argument: the sibling
/// operand reads the element's value before the call on both, so the two
/// lines agree, and both differ from the input.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn kernel_out_helper_reading_its_lent_argument_matches_the_host() {
    let result = miri_run(
        "
use system.gpu
use system.collections.array

fn add(v int, x out int)
    x = x + v

fn main()
    gpu var dst = [1, 2, 3, 4]
    gpu forall i in 0..4
        add(dst[i], dst[i])
    let h = dst
    println(f'{h[0]} {h[1]} {h[2]} {h[3]}')
    var c = [1, 2, 3, 4]
    for i in 0..4
        add(c[i], c[i])
    println(f'{c[0]} {c[1]} {c[2]} {c[3]}')
",
    );
    assert!(result.success, "{}", result.output());
    let lines: Vec<&str> = result.stdout.lines().collect();
    let [device, host] = lines.as_slice() else {
        panic!("a device line and a host line:\n{}", result.output());
    };
    assert_eq!(device, host, "the device agrees with the host");
    assert_ne!(*device, "1 2 3 4", "the helper wrote the element");
}
