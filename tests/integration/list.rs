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

// ==================== Methods ====================

#[test]
fn list_push_pop() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List()
l.push(10)
l.push(20)
println(f\"{l.length()}\")
println(f\"{l.pop()}\")
println(f\"{l.length()}\")
",
        "2\n20\n1",
    );
}

#[test]
fn list_insert_remove_at() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([1, 3])
l.insert(1, 2)
println(f\"{l[0]} {l[1]} {l[2]}\")
println(f\"{l.remove_at(1)}\")
println(f\"{l.length()}\")
",
        "1 2 3\n2\n2",
    );
}

#[test]
fn list_remove_by_value() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([10, 20, 30])
println(f\"{l.remove(20)}\")
println(f\"{l.remove(99)}\")
println(f\"{l.length()}\")
println(f\"{l[1]}\")
",
        "true\nfalse\n2\n30",
    );
}

#[test]
fn list_clear() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([1, 2, 3])
l.clear()
println(f\"{l.length()}\")
println(f\"{l.is_empty()}\")
",
        "0\ntrue",
    );
}

#[test]
fn list_reverse() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([1, 2, 3])
l.reverse()
println(f\"{l[0]} {l[1]} {l[2]}\")
",
        "3 2 1",
    );
}

// ==================== BaseList Methods ====================

#[test]
fn list_baselist_queries() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([10, 20, 30])
println(f\"{l.first() ?? -1}\")
println(f\"{l.last() ?? -1}\")
println(f\"{l.contains(20)}\")
println(f\"{l.index_of(30)}\")
println(f\"{l.last_index()}\")
",
        "10\n30\ntrue\n2\n2",
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
