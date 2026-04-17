// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ── Drop-fn setter wiring (task 2.4) ─────────────────────────────────────────

#[test]
fn test_array_of_strings_no_crash() {
    // Array<String>: elem_drop_fn must be registered so that miri_rt_array_free
    // DecRefs each string element. Verifies the setter call is emitted.
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let a = ["hello", "world"]
    println(f"{a[0]}")
"#,
        "hello",
    );
}

#[test]
fn test_array_of_strings_reassign_no_crash() {
    // Reassigning an Array<String> must free the old array and its string elements.
    // Both arrays must have the same size (arrays are fixed-size in Miri).
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var a = ["alpha", "beta"]
    a = ["gamma", "delta"]
    println(f"{a[0]}")
"#,
        "gamma",
    );
}

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
