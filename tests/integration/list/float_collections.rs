// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_list_float_push() {
    assert_runs_with_output(
        r#"
use system.collections.list

var a = List<float>()
a.push(2.5)
println(f'{a[0]}')
        "#,
        "2.5",
    );
}

#[test]
#[ignore = "f32 collection elements are refused at codegen: the element stride \
            for floats narrower than a word is unresolved, so a stored value \
            would read back as zero. float (f64) round-trips correctly."]
fn test_list_f32_push() {
    assert_runs_with_output(
        r#"
use system.collections.list

var a = List<f32>()
a.push(1.5)
println(f'{a[0]}')
        "#,
        "1.5",
    );
}

#[test]
fn test_list_float_multiple_push() {
    assert_runs_with_output(
        r#"
use system.collections.list

var a = List<float>()
a.push(1.0)
a.push(2.0)
a.push(3.0)
println(f'{a[0]}')
println(f'{a[1]}')
println(f'{a[2]}')
        "#,
        "1.0",
    );
}

#[test]
fn test_list_int_push_regression() {
    assert_runs_with_output(
        r#"
use system.collections.list

var a = List<int>()
a.push(5)
println(f'{a[0]}')
        "#,
        "5",
    );
}

#[test]
fn test_list_float_literal_construction() {
    assert_runs_with_output(
        r#"
use system.collections.list

var lst = [0.0, 0.0]
println(f'{lst[0]}')
        "#,
        "0.0",
    );
}

#[test]
fn test_list_float_literal_assignment() {
    assert_runs_with_output(
        r#"
use system.collections.list

var lst = [0.0, 0.0]
lst[0] = 5.0
println(f'{lst[0]}')
        "#,
        "5.0",
    );
}

#[test]
fn test_list_float_set_method() {
    assert_runs_with_output(
        r#"
use system.collections.list

var b = [0.0, 0.0]
b.set(0, 2.5)
println(f'{b[0]}')
        "#,
        "2.5",
    );
}

#[test]
#[ignore = "f32 collection elements are refused at codegen: the element stride \
            for floats narrower than a word is unresolved, so a stored value \
            would read back as zero. float (f64) round-trips correctly."]
fn test_list_f32_set_method() {
    assert_runs_with_output(
        r#"
use system.collections.list

var a = List<f32>()
a.push(0.0)
a.push(0.0)
a.set(0, 3.5)
println(f'{a[0]}')
        "#,
        "3.5",
    );
}

/// A float written through a chained index reaches the inner element intact.
///
/// The written value is read back through a binding to the inner list rather
/// than through a second chained index, because reading `a[0][0]` in one
/// expression corrupts the value independently of how it was stored — see
/// `test_nested_list_float_chained_read`.
#[test]
fn test_nested_list_float_write() {
    assert_runs_with_output(
        r#"
use system.collections.list
var a = [[0.0]]
a[0][0] = 2.5
let inner = a[0]
println(f'{inner[0]}')
        "#,
        "2.5",
    );
}

#[test]
fn test_nested_list_float_write_at_second_index() {
    assert_runs_with_output(
        r#"
use system.collections.list
var a = [[0.0, 1.0], [2.0, 3.0]]
a[1][0] = 9.5
let inner = a[1]
println(f'{inner[0]}')
        "#,
        "9.5",
    );
}

#[test]
fn test_nested_list_integer_write() {
    assert_runs_with_output(
        r#"
use system.collections.list
var a = [[0], [0]]
a[0][0] = 5
println(f'{a[0][0]}')
        "#,
        "5",
    );
}

/// Reading a float through two chained index expressions yields the element's
/// bit pattern reinterpreted as an integer: `[[1.5]]` read as `a[0][0]` gives
/// 1069547520.0, which is the f32 encoding of 1.5. Binding the inner list first
/// reads the same element correctly, and the integer element type is unaffected,
/// so the defect is in the width the chained read loads, not in the stored data.
#[test]
#[ignore = "a chained index read of a float element loads the wrong width and returns the bit pattern as an integer"]
fn test_nested_list_float_chained_read() {
    assert_runs_with_output(
        r#"
use system.collections.list
var a = [[1.5]]
println(f'{a[0][0]}')
        "#,
        "1.5",
    );
}
