// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_compiler_error, assert_runs, assert_runs_with_output};

// =========================================================================
// Construction
// =========================================================================

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

// =========================================================================
// Indexing
// =========================================================================

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

// =========================================================================
// For-loops
// =========================================================================

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
    println(f"{s}")
    "#,
        "a\nb\n",
    );
}

// =========================================================================
// F-string formatting
// =========================================================================

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

// =========================================================================
// BaseList methods
// =========================================================================

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
