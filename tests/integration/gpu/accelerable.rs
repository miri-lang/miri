// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// `Accelerable` capability gate: a `gpu let` / `gpu var` binding is admitted
// only when its type implements the stdlib `Accelerable` trait. Dispatch is by
// trait, never by stdlib type name — adding a GPU-eligible container is an
// `.mi` edit, not a compiler edit.

use super::utils::*;

#[test]
fn gpu_let_string_is_rejected() {
    assert_compiler_error(
        "
fn main()
    gpu let s = \"hello\"
",
        "'String' does not implement 'Accelerable' and cannot be gpu-resident.",
    );
}

#[test]
fn gpu_let_map_is_rejected() {
    assert_compiler_error(
        "
use system.collections.map

fn main()
    gpu let m = {1: 2}
",
        "'Map(int, int)' does not implement 'Accelerable' and cannot be gpu-resident.",
    );
}

#[test]
fn gpu_let_set_is_rejected() {
    assert_compiler_error(
        "
use system.collections.set

fn main()
    gpu let s = {1, 2, 3}
",
        "does not implement 'Accelerable' and cannot be gpu-resident.",
    );
}

#[test]
fn gpu_let_array_literal_is_accepted() {
    assert_type_checks(
        "
use system.collections.array

fn main()
    gpu let g = [1, 2, 3]
",
    );
}

#[test]
fn gpu_let_list_is_accepted() {
    assert_type_checks(
        "
use system.collections.list

fn main()
    gpu let g = List([1, 2, 3])
",
    );
}

#[test]
fn gpu_let_tuple_of_scalars_is_accepted() {
    assert_type_checks(
        "
fn main()
    gpu let t = (1, 2)
",
    );
}

#[test]
fn gpu_var_scalar_is_accepted() {
    assert_type_checks(
        "
fn main()
    gpu var x = 0
",
    );
}

#[test]
fn gpu_let_float_array_is_accepted() {
    assert_type_checks(
        "
use system.collections.array

fn main()
    gpu let g = [1.5, 2.5, 3.5]
",
    );
}

#[test]
fn gpu_let_empty_array_is_rejected() {
    assert_compiler_error(
        "
use system.collections.array

fn main()
    gpu let g = []
",
        "does not implement 'Accelerable' and cannot be gpu-resident.",
    );
}

// The headline property: a brand-new user type the compiler has never heard of
// becomes gpu-eligible purely by declaring `implements Accelerable` — no compiler
// edit. This demonstrates the `.mi`-extensibility principle: new container types
// can be made GPU-eligible without modifying the compiler.
#[test]
fn gpu_let_user_type_implementing_accelerable_is_accepted() {
    assert_type_checks(
        "
use system.accelerator

class Particle implements Accelerable
    x int

fn main()
    gpu let p = Particle(1)
",
    );
}

// A type cannot declare `implements Accelerable` unless every field is itself
// accelerable — the capability must reflect reality, not be slapped on a type
// that can never live on a device.
#[test]
fn implementing_accelerable_with_non_accelerable_field_is_rejected() {
    assert_compiler_error(
        "
use system.accelerator

class Bad implements Accelerable
    s String

fn main()
    let x = 1
",
        "field 's' has type 'String', which is not accelerable",
    );
}

#[test]
fn implementing_accelerable_with_map_field_is_rejected() {
    assert_compiler_error(
        "
use system.accelerator
use system.collections.map

class Bad implements Accelerable
    m Map<int, int>

fn main()
    let x = 1
",
        "which is not accelerable",
    );
}

#[test]
fn implementing_accelerable_with_generic_field_is_accepted() {
    assert_type_checks(
        "
use system.accelerator

class Box<T> implements Accelerable
    val T

fn main()
    gpu let b = Box<int>(1)
",
    );
}

#[test]
fn generic_accelerable_with_non_accelerable_type_arg_is_rejected() {
    assert_compiler_error(
        "
use system.accelerator

class Box<T> implements Accelerable
    val T

fn main()
    gpu let b = Box<String>(\"x\")
",
        "does not implement 'Accelerable' and cannot be gpu-resident.",
    );
}

#[test]
fn gpu_let_user_type_without_accelerable_is_rejected() {
    assert_compiler_error(
        "
class Plain
    x int

fn main()
    gpu let p = Plain(1)
",
        "'Plain' does not implement 'Accelerable' and cannot be gpu-resident.",
    );
}

// A `bool` array cannot back a WGSL storage buffer (WGSL forbids `bool` in
// `var<storage>`), so a gpu-resident binding over it must be rejected at the
// binding site — not silently admitted here and only flagged later at a `gpu
// forall` capture. This is the coherence guarantee: the binding gate and the
// buffer-element gate agree on the element set.
#[test]
fn gpu_let_bool_array_is_rejected_at_binding() {
    assert_compiler_error(
        "
use system.collections.array

fn main()
    gpu let g = [true, false]
",
        "does not implement 'Accelerable' and cannot be gpu-resident.",
    );
}

#[test]
fn host_string_binding_is_unaffected() {
    assert_type_checks(
        "
fn main()
    let s = \"hello\"
",
    );
}
