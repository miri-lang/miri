// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// Integration tests for List construction, indexing, for-loops, and bounds checking.
// Note: `[1, 2, 3]` creates an Array. List is created via `List([1, 2, 3])`.

use super::utils::*;

// ==================== Construction ====================

#[test]
fn list_construction_int() {
    assert_runs(
        "
use system.collections.list

let l = List([1, 2, 3])
",
    );
}

#[test]
fn list_construction_string() {
    assert_runs(
        "
use system.collections.list

let l = List([\"hello\", \"world\"])
",
    );
}

// ==================== Indexing ====================

#[test]
fn list_indexing() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([10, 20, 30])
println(f\"{l[0]}\")
println(f\"{l[1]}\")
println(f\"{l[2]}\")
",
        "10\n20\n30",
    );
}

#[test]
fn list_indexing_first_element() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([42, 99])
println(f\"{l[0]}\")
",
        "42",
    );
}

// ==================== Length ====================

#[test]
fn list_length_via_fstring() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([1, 2, 3, 4, 5])
println(f\"{l.length()}\")
",
        "5",
    );
}

// ==================== For-loop ====================

#[test]
fn list_for_loop() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([1, 2, 3])
for x in l
    println(f\"{x}\")
",
        "1\n2\n3",
    );
}

#[test]
fn list_for_loop_with_index() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([10, 20, 30])
for x, idx in l
    println(f\"{idx} = {x}\")
",
        "0 = 10\n1 = 20\n2 = 30",
    );
}

// ==================== F-string ====================

#[test]
fn list_fstring_element() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([7, 8, 9])
println(f\"{l[1]}\")
",
        "8",
    );
}

// ==================== Runtime bounds check ====================

#[test]
fn list_runtime_oob_crash() {
    assert_runtime_crash(
        "
use system.io
use system.collections.list

let l = List([1, 2, 3])
var idx = 10
println(f\"{l[idx]}\")
",
    );
}
