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

// ==================== Sort ====================

#[test]
fn list_sort() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([30, 10, 20, 5])
l.sort()
println(f\"{l[0]} {l[1]} {l[2]} {l[3]}\")
",
        "5 10 20 30",
    );
}

#[test]
fn list_sort_already_sorted() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([1, 2, 3])
l.sort()
println(f\"{l[0]} {l[1]} {l[2]}\")
",
        "1 2 3",
    );
}

#[test]
fn list_sort_reverse_order() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([5, 4, 3, 2, 1])
l.sort()
println(f\"{l[0]} {l[1]} {l[2]} {l[3]} {l[4]}\")
",
        "1 2 3 4 5",
    );
}

// ==================== Additional Method Tests ====================

#[test]
fn list_get_method() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([10, 20, 30])
println(f\"{l.get(0)}\")
println(f\"{l.get(2)}\")
",
        "10\n30",
    );
}

#[test]
fn list_set_method() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([10, 20, 30])
l.set(1, 99)
println(f\"{l[0]} {l[1]} {l[2]}\")
",
        "10 99 30",
    );
}

#[test]
fn list_is_empty_false() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([1])
println(f\"{l.is_empty()}\")
",
        "false",
    );
}

#[test]
fn list_last_index_method() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([10, 20, 30])
println(f\"{l.last_index()}\")
",
        "2",
    );
}

#[test]
fn list_first_last_on_single_element() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([42])
println(f\"{l.first() ?? -1}\")
println(f\"{l.last() ?? -1}\")
",
        "42\n42",
    );
}

#[test]
fn list_contains_false() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([1, 2, 3])
println(f\"{l.contains(99)}\")
",
        "false",
    );
}

#[test]
fn list_index_of_not_found() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([1, 2, 3])
println(f\"{l.index_of(99)}\")
",
        "-1",
    );
}

#[test]
fn list_element_at() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([10, 20, 30])
println(f\"{l.element_at(1)}\")
",
        "20",
    );
}

#[test]
fn list_push_multiple_then_iterate() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List()
l.push(1)
l.push(2)
l.push(3)
for x in l
    println(f\"{x}\")
",
        "1\n2\n3",
    );
}

// ==================== Function Integration Tests ====================

#[test]
fn list_passed_to_function() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

fn sum_list(l [int]) int
    var total = 0
    for x in l
        total += x
    return total

fn main()
    let l = List([10, 20, 30])
    println(f\"{sum_list(l: l)}\")
",
        "60",
    );
}

#[test]
fn list_returned_from_function() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

fn make_list() [int]
    List([1, 2, 3])

fn main()
    let l = make_list()
    println(f\"{l[0]} {l[1]} {l[2]}\")
",
        "1 2 3",
    );
}

#[test]
fn list_in_struct_field() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

struct Data
    items [int]

fn main()
    let d = Data(items: List([10, 20, 30]))
    println(f\"{d.items[0]} {d.items[1]} {d.items[2]}\")
",
        "10 20 30",
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

// ==================== RC Aliasing ====================

#[test]
fn test_list_alias_no_double_free() {
    assert_runs(
        "
use system.collections.list
let l1 = List([1, 2, 3])
let l2 = l1 // IncRef
// Both out of scope, safe drop
",
    );
}

#[test]
fn test_list_reassign_frees_old() {
    assert_runs(
        "
use system.collections.list
var l = List([1, 2, 3])
l = List([4, 5])
",
    );
}

#[test]
fn test_list_passed_to_function_no_dangle() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

fn consume(l [int])
    // goes out of scope

fn main()
    let l = List([10, 20, 30])
    consume(l)
    println(f\"{l.length()}\")
",
        "3",
    );
}

#[test]
fn test_list_returned_from_function_with_rc() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

fn make_and_alias() [int]
    let l = List([1, 2, 3])
    let alias = l
    return alias

fn main()
    let l = make_and_alias()
    println(f\"{l.length()}\")
",
        "3",
    );
}
