// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_array_f32_index_assignment() {
    assert_runs_with_output(
        r#"
use system.collections.array

var g = Array<f32, 3>()
g[0] = 5.0
println(f'{g[0]}')
        "#,
        "5.0",
    );
}

#[test]
fn test_array_float_index_assignment() {
    assert_runs_with_output(
        r#"
use system.collections.array

var d = Array<float, 2>()
d[0] = 2.5
println(f'{d[0]}')
        "#,
        "2.5",
    );
}

#[test]
fn test_array_f32_literal_assignment() {
    assert_runs_with_output(
        r#"
use system.collections.array

var g = Array<f32, 3>()
g[0] = 1.5
println(f'{g[0]}')
        "#,
        "1.5",
    );
}

#[test]
fn test_array_f32_variable_rhs() {
    assert_runs_with_output(
        r#"
use system.collections.array

var x = 5.0
var g = Array<f32, 3>()
g[0] = x
println(f'{g[0]}')
        "#,
        "5.0",
    );
}

#[test]
fn test_array_int_assignment_regression() {
    assert_runs_with_output(
        r#"
use system.collections.array

var ints = Array<int, 2>()
ints[0] = 5
println(f'{ints[0]}')
        "#,
        "5",
    );
}

#[test]
fn test_array_float_comparison() {
    assert_runs_with_output(
        r#"
use system.collections.array

var g = Array<f32, 3>()
g[0] = 5.0
if g[0] > 1.0:
    println("greater")
else:
    println("not_greater")
        "#,
        "greater",
    );
}

#[test]
fn test_array_float_arithmetic() {
    assert_runs_with_output(
        r#"
use system.collections.array

var g = Array<f32, 3>()
g[0] = 2.0
var x = g[0] * 2.0
println(f'{x}')
        "#,
        "4.0",
    );
}

#[test]
fn test_array_nested_float_write_via_literal() {
    assert_runs_with_output(
        r#"
use system.collections.array

var inner = Array<f32, 1>()
inner[0] = 2.5
var outer = Array<f32, 1>()
outer[0] = inner[0]
println(f'{outer[0]}')
        "#,
        "2.5",
    );
}
