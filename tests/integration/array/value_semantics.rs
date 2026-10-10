// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An array is a value: a second binding shares its buffer until either one
//! writes, and the write copies a buffer still shared before changing it.

use super::utils::assert_heap_guard_output;

#[test]
fn test_index_write_to_a_rebound_array_leaves_the_original_alone() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    var a = [1, 2, 3]
    var b = a
    b[0] = 9
    a[2] += 10
    println(f"{a[0]} {a[2]} {b[0]} {b[2]}")
"#,
        "1 13 9 3",
    );
}

#[test]
fn test_mutating_methods_on_a_rebound_array_leave_the_original_alone() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let a = [3, 1, 2]
    var b = a
    b.set(0, 7)
    var c = a
    c.sort()
    var d = a
    d.reverse()
    println(f"{a[0]} {a[1]} {a[2]} {b[0]} {c[0]} {d[0]}")
"#,
        "3 1 2 7 1 2",
    );
}

#[test]
fn test_array_of_strings_written_through_a_copy_keeps_both() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    var a = ["x", "y"]
    var b = a
    b[1] = "z"
    println(f"{a[1]} {b[1]}")
"#,
        "y z",
    );
}

#[test]
fn test_array_captured_by_a_closure_keeps_its_value() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    var a = [1, 2]
    let f = fn() int: a[0]
    a[0] = 5
    println(f"{f()} {a[0]}")
"#,
        "1 5",
    );
}
