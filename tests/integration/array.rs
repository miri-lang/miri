// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_compiler_error, assert_runs, assert_runs_with_output};

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
fn test_array_indexing() {
    assert_runs_with_output(
        r#"
use system.io

let a = [10, 20, 30]
print(f"{a[1]}")
    "#,
        "20",
    );
}

#[test]
fn test_array_first_element() {
    assert_runs_with_output(
        r#"
use system.io

let a = [1, 2, 3]
print(f"{a[0]}")
    "#,
        "1",
    );
}

#[test]
fn test_array_last_element() {
    assert_runs_with_output(
        r#"
use system.io

let a = [1, 2, 3]
print(f"{a[2]}")
    "#,
        "3",
    );
}

#[test]
fn test_array_variable_index() {
    assert_runs_with_output(
        r#"
use system.io

let i = 1
let a = [10, 20, 30]
print(f"{a[i]}")
    "#,
        "20",
    );
}

#[test]
fn test_array_index_assignment() {
    assert_runs_with_output(
        r#"
use system.io

var a = [10, 20, 30]
a[1] = 99
print(f"{a[1]}")
    "#,
        "99",
    );
}

#[test]
fn test_array_for_loop() {
    assert_runs_with_output(
        r#"
use system.io

for x in [1, 2, 3]
    println(f"{x}")
    "#,
        "1\n2\n3\n",
    );
}

#[test]
fn test_array_for_loop_strings() {
    assert_runs_with_output(
        r#"
use system.io

for s in ["a", "b"]
    println(s)
    "#,
        "a\nb\n",
    );
}

#[test]
fn test_array_fstring() {
    assert_runs_with_output(
        r#"
use system.io

print(f"{[1, 2, 3][0]}")
    "#,
        "1",
    );
}

#[test]
fn test_array_baselist_methods() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

let a = [10, 20, 30]
println(f"{a.first() ?? -1}")
println(f"{a.last() ?? -1}")
println(f"{a.is_empty()}")
println(f"{a.contains(20)}")
println(f"{a.index_of(30)}")
"#,
        "10\n30\nfalse\ntrue\n2",
    );
}

#[test]
fn test_array_reverse() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

let a = [10000000000, 20000000000, 30000000000]
a.reverse()
println(f"{a[0]} {a[1]} {a[2]}")
"#,
        "30000000000 20000000000 10000000000",
    );
}

// ==================== Method Tests ====================

#[test]
fn test_array_set_method() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

let a = [10, 20, 30]
a.set(1, 99)
println(f"{a[0]} {a[1]} {a[2]}")
"#,
        "10 99 30",
    );
}

#[test]
fn test_array_length_method() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

let a = [1, 2, 3, 4, 5]
println(f"{a.length()}")
"#,
        "5",
    );
}

#[test]
fn test_array_element_at() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

let a = [10, 20, 30]
println(f"{a.element_at(1)}")
"#,
        "20",
    );
}

#[test]
fn test_array_is_empty_false() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

let a = [1]
println(f"{a.is_empty()}")
"#,
        "false",
    );
}

#[test]
fn test_array_first_last_methods() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

let a = [100, 200, 300]
println(f"{a.first() ?? -1}")
println(f"{a.last() ?? -1}")
"#,
        "100\n300",
    );
}

#[test]
fn test_array_contains_true_and_false() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

let a = [5, 10, 15]
println(f"{a.contains(10)}")
println(f"{a.contains(99)}")
"#,
        "true\nfalse",
    );
}

#[test]
fn test_array_index_of_found_and_not_found() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

let a = [5, 10, 15]
println(f"{a.index_of(15)}")
println(f"{a.index_of(99)}")
"#,
        "2\n-1",
    );
}

#[test]
fn test_array_reverse_two_elements() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

let a = [1, 2]
a.reverse()
println(f"{a[0]} {a[1]}")
"#,
        "2 1",
    );
}

#[test]
fn test_array_sort() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

let a = [30, 10, 20, 5]
a.sort()
println(f"{a[0]} {a[1]} {a[2]} {a[3]}")
"#,
        "5 10 20 30",
    );
}

#[test]
fn test_array_sort_already_sorted() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

let a = [1, 2, 3]
a.sort()
println(f"{a[0]} {a[1]} {a[2]}")
"#,
        "1 2 3",
    );
}

#[test]
fn test_array_sort_reverse_order() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

let a = [5, 4, 3, 2, 1]
a.sort()
println(f"{a[0]} {a[1]} {a[2]} {a[3]} {a[4]}")
"#,
        "1 2 3 4 5",
    );
}

// ==================== Function Integration Tests ====================

#[test]
fn test_array_returned_from_function() {
    assert_runs_with_output(
        r#"
use system.io

fn make_array() [int; 3]
    [10, 20, 30]

fn main()
    let a = make_array()
    println(f"{a[0]} {a[1]} {a[2]}")
"#,
        "10 20 30",
    );
}

#[test]
fn test_array_in_struct_field() {
    assert_runs_with_output(
        r#"
use system.io

struct Data
    values [int; 3]

fn main()
    let d = Data(values: [10, 20, 30])
    println(f"{d.values[0]} {d.values[1]} {d.values[2]}")
"#,
        "10 20 30",
    );
}

// =========================================================================
// Compile-time error tests
// =========================================================================

#[test]
fn test_array_index_out_of_bounds_literal() {
    assert_compiler_error(
        r#"
let a = [1, 2, 3]
let x = a[5]
    "#,
        "Array index out of bounds",
    );
}

#[test]
fn test_array_mixed_types() {
    assert_compiler_error(
        r#"
let a = [1, "hello"]
    "#,
        "Array elements must have the same type",
    );
}

#[test]
fn test_array_non_int_index() {
    assert_compiler_error(
        r#"
let a = [1, 2, 3]
let x = a["x"]
    "#,
        "Array index must be an integer",
    );
}

// ==================== RC / Aliasing Tests ====================

#[test]
fn test_array_alias_no_double_free() {
    // Two variables pointing at the same array should not double-free.
    // RC is incremented on alias, decremented when each goes out of scope.
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var a = [1, 2, 3]
    var b = a
    println(f"{b[0]}")
    "#,
        "1",
    );
}

#[test]
fn test_array_reassign_frees_old() {
    // Reassigning a collection variable should free the old value via DecRef.
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var a = [1, 2, 3]
    a = [4, 5, 6]
    println(f"{a[0]}")
    "#,
        "4",
    );
}

#[test]
fn test_array_passed_to_function_no_dangle() {
    // A collection passed to a function should not dangle after return.
    assert_runs_with_output(
        r#"
use system.io

fn sum_first(arr [int; 3]) int
    arr[0] + arr[1] + arr[2]

fn main()
    let a = [10, 20, 30]
    let s = sum_first(arr: a)
    println(f"{s}")
    "#,
        "60",
    );
}

// ==================== elem_size Tests ====================

#[test]
fn test_array_of_structs_elem_size() {
    // Array<Point> where Point is a struct — elements are pointer-sized
    // because structs are heap-allocated.
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn main()
    let p1 = Point(x: 1, y: 2)
    let p2 = Point(x: 3, y: 4)
    let arr = [p1, p2]
    let first = arr[0]
    println(f"{first.x}")
    println(f"{first.y}")
    let second = arr[1]
    println(f"{second.x}")
    "#,
        "1\n2\n3",
    );
}

#[test]
fn test_nested_arrays() {
    // Nested arrays: inner arrays are pointer-sized elements.
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let a = [10, 20, 30]
    let b = [40, 50, 60]
    let nested = [a, b]
    let inner = nested[1]
    println(f"{inner[2]}")
    "#,
        "60",
    );
}
