// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_array_creation() {
    assert_runs("let a = [1, 2, 3]");
}

#[test]
fn test_array_single_element() {
    assert_runs("let a = [42]");
}

#[test]
fn test_array_strings() {
    assert_runs(r#"let a = ["hello", "world"]"#);
}

#[test]
fn test_array_booleans() {
    assert_runs("let a = [true, false, true]");
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
