// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn list_runtime_oob_crash() {
    assert_runtime_crash(
        "
use system.collections.list

let l = List([1, 2, 3])
var idx = 10
println(f\"{l[idx]}\")
",
    );
}

#[test]
fn list_negative_index_reports_signed_index() {
    // A negative index is out of bounds, but the error must report its actual
    // signed value (`-1`), not the huge wrapped `usize` (18446744073709551615)
    // it becomes when reinterpreted as unsigned.
    assert_runtime_error(
        "
use system.collections.list

let l = List([1, 2, 3])
let i = 0 - 1
println(f\"{l[i]}\")
",
        "the index is -1",
    );
}

#[test]
fn list_out_of_bounds_set() {
    assert_runtime_crash(
        "
use system.collections.list

let l = List([1, 2])
l.set(5, 99)
",
    );
}

#[test]
fn list_constructor_rejects_set_arg() {
    // `List(<set>)` previously type-checked but crashed at runtime (SIGBUS) because
    // lowering treated the set pointer as a raw array. The type checker now
    // rejects any non-sequence argument.
    assert_compiler_error(
        "
use system.collections.list

let l = List({1, 2, 3})
",
        "List(...) expects an array or list argument",
    );
}

#[test]
fn list_constructor_rejects_scalar_arg() {
    assert_compiler_error(
        "
use system.collections.list

let l = List(42)
",
        "List(...) expects an array or list argument",
    );
}

#[test]
fn list_with_type_argument_rejects_scalar_arg() {
    // Writing the element type must not skip the argument check: lowering hands
    // the argument to the list-copy routine, which reads it as a sequence
    // header, so a scalar there is a wild read.
    assert_compiler_error(
        "
use system.collections.list

fn main()
    let five = List<int>(5)
    println(f\"{five.length()}\")
",
        "expects an array or list of 'int'",
    );
}

#[test]
fn list_with_type_argument_rejects_mismatched_element_type() {
    // The elements are string pointers; reading them as `int` prints addresses.
    assert_compiler_error(
        "
use system.collections.list

fn main()
    let strs = List([\"PEAR\".to_lower()])
    let wrong = List<int>(strs)
    println(f\"{wrong[0]}\")
",
        "expects an array or list of 'int'",
    );
}

#[test]
fn list_with_type_argument_rejects_set_arg() {
    assert_compiler_error(
        "
use system.collections.list

fn main()
    let l = List<int>({1, 2, 3})
    println(f\"{l.length()}\")
",
        "expects an array or list of 'int'",
    );
}

// ── Slice a non-sliceable type ──────────────────────────────────────────

#[test]
fn struct_slice_is_not_sliceable() {
    assert_compiler_error(
        "
struct Point
    x int
    y int

let p = Point { x: 1, y: 2 }
let slice = p[0..1]
",
        "is not sliceable",
    );
}

#[test]
fn enum_slice_is_not_sliceable() {
    assert_compiler_error(
        "
enum Color
    Red
    Blue

let c = Color.Red
let slice = c[0..1]
",
        "is not sliceable",
    );
}

#[test]
fn scalar_slice_is_not_sliceable() {
    assert_compiler_error(
        "
let x = 42
let slice = x[0..1]
",
        "is not sliceable",
    );
}

#[test]
fn list_with_type_argument_rejects_a_narrower_element_width() {
    // The elements are laid out four bytes apart; copying them word for word
    // into an `int` list reads two of them as one element (8589934593).
    assert_compiler_error(
        "
use system.collections.list

fn main()
    let src = List<i32>([1, 2, 3])
    let wide = List<int>(src)
    println(f\"{wide[0]}\")
",
        "expects an array or list of 'int'",
    );
}
