// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_array_nested_modification() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let row1 = [1, 2]
    var row2 = [3, 4]
    var nested = [row1, row2]
    
    // Modify the nested array
    nested[1][0] = 99
    
    let modified_row = nested[1]
    println(f"{modified_row[0]} {modified_row[1]}")
    "#,
        "99 4",
    );
}

#[test]
fn test_array_zero_length_methods() {
    // Note: Empty array literal syntax. If this compilation fails because empty arrays are not supported,
    // this test will correctly capture that as a missing language feature or a type checker bound.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

fn main()
    let a [int; 0] = []
    println(f"{a.length()}")
    println(f"{a.is_empty()}")
    "#,
        "0\ntrue",
    );
}

#[test]
fn test_array_deep_nested_sort_and_reverse() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

fn main()
    // a is a 2D array: [ [3, 2, 4], [8, 7, 5] ]
    let row1 = [3, 2, 4]
    let row2 = [8, 7, 5]
    let a = [row1, row2]
    
    // Sort and Reverse the inner arrays
    a[0].sort()
    a[1].reverse()
    
    let modified1 = a[0]
    let modified2 = a[1]
    println(f"{modified1[0]} {modified1[1]} {modified1[2]} {modified2[0]} {modified2[1]} {modified2[2]}")
    "#,
        "2 3 4 5 7 8",
    );
}

#[test]
fn test_array_of_optional_elements_methods() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

fn main()
    let opt_none int? = None
    var opt_five int? = None
    opt_five = 5
    let a [int?; 3] = [opt_five, opt_none, opt_none]
    
    // Use an explicit if-else or `??` to check `first()` since `first()` on `[T]` returns `T?`.
    // Here `T` is `int?`, so `first()` returns `int??`. Miri may collapse or require handling.
    let f1 = a.first()
    let l1 = a.last()
    
    let contains_none = a.contains(opt_none)
    let index_of_none = a.index_of(opt_none)
    
    println(f"{contains_none} {index_of_none}")
    "#,
        "true 1",
    );
}

#[test]
fn test_array_deep_nested_contains_and_index_of() {
    // Tests if `==` logic properly compares struct/array types, or if it errors out correctly during codegen.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

fn main()
    let a = [[1, 2], [3, 4]]
    let search_target = [3, 4]
    let c = a.contains(search_target)
    let idx = a.index_of(search_target)
    
    println(f"{c} {idx}")
    "#,
        "false -1",
    );
}

#[test]
fn test_array_reverse_odd_and_even_lengths() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

fn main()
    let a = [1, 2, 3]
    let b = [10, 20, 30, 40]
    
    a.reverse()
    b.reverse()
    
    println(f"{a[0]} {a[1]} {a[2]}")
    println(f"{b[0]} {b[1]} {b[2]} {b[3]}")
    "#,
        "3 2 1\n40 30 20 10",
    );
}

#[test]
fn test_array_element_at_and_set_complex_expressions() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

fn compute_offset() int
    1

fn main()
    var a = [10, 20, 30, 40]
    
    // a.set(1 + 1, a.element_at(1 * 0) + 100) -> a.set(2, a.element_at(0) + 100) -> a.set(2, 110)
    a.set(1 + compute_offset(), a.element_at(0) + 100)
    
    println(f"{a[0]} {a[2]} {a[3]}")
    "#,
        "10 110 40",
    );
}
