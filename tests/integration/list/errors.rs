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
fn list_out_of_bounds_remove_at() {
    assert_runtime_crash(
        "
use system.collections.list

let l = List([1, 2])
l.remove_at(5)
",
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
    // rejects any non-array argument.
    assert_compiler_error(
        "
use system.collections.list

let l = List({1, 2, 3})
",
        "List(...) expects an array literal argument",
    );
}

#[test]
fn list_constructor_rejects_scalar_arg() {
    assert_compiler_error(
        "
use system.collections.list

let l = List(42)
",
        "List(...) expects an array literal argument",
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
