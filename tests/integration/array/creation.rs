// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_array_creation() {
    assert_runs_with_output(
        "use system.collections.array\nlet a = [1, 2, 3]\nprintln(f\"{a.length()}\")",
        "3",
    );
}

#[test]
fn test_array_single_element() {
    assert_runs_with_output(
        "use system.collections.array\nlet a = [42]\nprintln(f\"{a.length()}\")",
        "1",
    );
}

#[test]
fn test_array_strings() {
    assert_runs_with_output(
        "use system.collections.array\nlet a = [\"hello\", \"world\"]\nprintln(f\"{a.length()}\")",
        "2",
    );
}

#[test]
fn test_array_booleans() {
    assert_runs_with_output(
        "use system.collections.array\nlet a = [true, false, true]\nprintln(f\"{a.length()}\")",
        "3",
    );
}

#[test]
fn test_array_sized_int() {
    assert_runs_with_output(
        "
use system.collections.array

let a = Array<int, 8>()
println(f\"{a.length()}\")
",
        "8",
    );
}

#[test]
fn test_array_sized_u32() {
    assert_runs_with_output(
        "
use system.collections.array

let a = Array<u32, 5>()
println(f\"{a.length()}\")
",
        "5",
    );
}

#[test]
fn test_array_sized_f32() {
    assert_runs_with_output(
        "
use system.collections.array

let a = Array<f32, 4>()
println(f\"{a.length()}\")
",
        "4",
    );
}

#[test]
fn test_array_sized_arithmetic() {
    assert_runs_with_output(
        "
use system.collections.array

let a = Array<int, 4 * 4>()
println(f\"{a.length()}\")
",
        "16",
    );
}

#[test]
fn test_array_sized_zero_initialized() {
    assert_runs_with_output(
        "
use system.collections.array

let a = Array<int, 3>()
println(f\"{a[0]}\")
println(f\"{a[1]}\")
println(f\"{a[2]}\")
",
        "0\n0\n0",
    );
}

#[test]
fn test_array_sized_named_const() {
    assert_runs_with_output(
        "
use system.collections.array

const SIZE = 8
let a = Array<int, SIZE>()
println(f\"{a.length()}\")
",
        "8",
    );
}

#[test]
fn test_array_sized_named_const_arithmetic() {
    assert_runs_with_output(
        "
use system.collections.array

const SIZE = 4 * 4
let a = Array<int, SIZE>()
println(f\"{a.length()}\")
",
        "16",
    );
}

#[test]
fn test_array_sized_named_const_f32_buffer() {
    assert_runs_with_output(
        "
use system.collections.array

const SIZE = 64 * 64
let pixels = Array<f32, SIZE>()
println(f\"{pixels.length()}\")
",
        "4096",
    );
}

#[test]
fn test_array_sized_const_arithmetic_in_slot() {
    // Arithmetic over named `const`s directly in the value-generic slot of a
    // constructor call: the slot must accept a const-foldable expression, not
    // only a single literal or single named const.
    assert_runs_with_output(
        "
use system.collections.array

const W = 8
let a = Array<int, W * W>()
println(f\"{a.length()}\")
",
        "64",
    );
}

#[test]
fn test_array_sized_const_arithmetic_multi_factor_in_slot() {
    // A three-factor product (`W * H * 4`, an RGBA paint-buffer size) folds in
    // the constructor slot, exercising left-associative multiplicative chaining.
    assert_runs_with_output(
        "
use system.collections.array

const W = 4
const H = 3
let a = Array<f32, W * H * 4>()
println(f\"{a.length()}\")
",
        "48",
    );
}

#[test]
fn test_array_sized_const_arithmetic_mixed_precedence_in_slot() {
    // Additive and multiplicative mix in the slot: `W * W + 1` must respect
    // precedence (multiply binds tighter) → 8*8 + 1 = 65.
    assert_runs_with_output(
        "
use system.collections.array

const W = 8
let a = Array<int, W * W + 1>()
println(f\"{a.length()}\")
",
        "65",
    );
}

#[test]
fn test_array_sized_const_arithmetic_type_position() {
    // Type-position form of the same expression (`var a Array<..>`), pinned so
    // the constructor and type-position parses stay in lockstep.
    assert_runs_with_output(
        "
use system.collections.array

const W = 8
var a Array<int, W * W>
a = Array<int, W * W>()
println(f\"{a.length()}\")
",
        "64",
    );
}

#[test]
fn test_array_sized_named_const_struct_field_type() {
    // Type-position form: a named `const` in a struct-field `Array<T, N>`
    // must fold to the same literal the constructor form uses, so the
    // field type and the constructed value agree.
    assert_runs_with_output(
        "
use system.collections.array

const SIZE = 4

struct Buffer
    data Array<f32, SIZE>

let b = Buffer(data: Array<f32, SIZE>())
println(f\"{b.data.length()}\")
",
        "4",
    );
}

/// A value coerced into a differently-spelled type is released once, not twice.
///
/// The coercion re-spells the type in place, so the temp holds the same array the
/// variable does. Releasing both frees one allocation twice — silently for a single
/// run, and fatally once the freed block is handed out again.
#[test]
fn test_array_sized_const_coercion_releases_once() {
    assert_runs_with_output(
        "
use system.collections.array

const SIZE = 3

fn total(xs Array<int, SIZE>) int
    xs.length()

fn main()
    var i = 0
    var acc = 0
    while i < 300
        let a = Array<int, SIZE>()
        acc = acc + total(a)
        i = i + 1
    println(f\"{acc}\")
",
        "900",
    );
}

#[test]
fn test_array_sized_named_const_param_type() {
    // Type-position form in a function parameter type.
    assert_runs_with_output(
        "
use system.collections.array

const SIZE = 3

fn total(xs Array<int, SIZE>) int
    xs.length()

let a = Array<int, SIZE>()
println(f\"{total(a)}\")
",
        "3",
    );
}

#[test]
fn test_array_sized_named_const_param_and_return_type() {
    // Type-position form in both a parameter and a return type: a const-sized
    // array flows through a passthrough function unchanged.
    assert_runs_with_output(
        "
use system.collections.array

const SIZE = 5

fn passthrough(xs Array<int, SIZE>) Array<int, SIZE>
    xs

let a = Array<int, SIZE>()
let b = passthrough(a)
println(f\"{b.length()}\")
",
        "5",
    );
}

#[test]
fn test_array_sized_named_const_inside_function_body() {
    // `Array<T, SIZE>()` constructed inside a function body where the const is
    // declared later in source order. Top-level bindings are hoisted, so the
    // value-generic size folds even though the function is checked first.
    assert_runs_with_output(
        "
use system.collections.array

fn make() int
    let a = Array<int, SIZE>()
    a.length()

const SIZE = 4

println(f\"{make()}\")
",
        "4",
    );
}

#[test]
fn test_array_sized_named_const_mismatch_error() {
    // Folding the const in the size slot must not make every array compatible:
    // a field typed `Array<f32, A>` still rejects an `Array<f32, B>` value.
    assert_compiler_error(
        "
use system.collections.array

const A = 4
const B = 8

struct Buffer
    data Array<f32, A>

let b = Buffer(data: Array<f32, B>())
",
        "Type mismatch for field 'data'",
    );
}

#[test]
fn test_array_sized_non_const_error() {
    assert_compiler_error(
        "
use system.collections.array

var n = 5
let a = Array<int, n>()
",
        "compile-time constant",
    );
}

#[test]
fn test_array_sized_non_const_arithmetic_error() {
    // Arithmetic over a non-`const` binding is still not a compile-time
    // constant: the slot now parses `n * n` but the fold fails, so the size
    // check reports the constant-size diagnostic instead of crashing.
    assert_compiler_error(
        "
use system.collections.array

var n = 5
let a = Array<int, n * n>()
",
        "compile-time constant",
    );
}

#[test]
fn test_array_sized_managed_element_type_error() {
    assert_compiler_error(
        "
use system.collections.array

let a = Array<List<int>, 4>()
",
        "managed element type",
    );
}

#[test]
fn test_array_sized_zero_length() {
    assert_runs_with_output(
        "
use system.collections.array

let a = Array<int, 0>()
println(f\"{a.length()}\")
",
        "0",
    );
}

#[test]
fn test_array_sized_zero_elements() {
    assert_runs_with_output(
        "
use system.collections.array

let a = Array<f32, 0>()
if a.length() == 0
    println(\"zero elements\")
",
        "zero elements",
    );
}
