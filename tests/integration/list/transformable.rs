// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn list_map_doubles_elements() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list
use system.collections.transformable

let l = List([1, 2, 3])
let doubled = l.map(fn(x int) int: x * 2)
for item in doubled
    println(f\"{item}\")
",
        "2\n4\n6",
    );
}

#[test]
fn list_filter_keeps_positives() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list
use system.collections.transformable

let l = List([-1, 2, -3, 4, 0])
let pos = l.filter(fn(x int) bool: x > 0)
println(f\"{pos.length()}\")
for item in pos
    println(f\"{item}\")
",
        "2\n2\n4",
    );
}

#[test]
fn list_reduce_sums_elements() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list
use system.collections.transformable

let l = List([1, 2, 3, 4, 5])
let sum = l.reduce(0, fn(a int, b int) int: a + b)
println(f\"{sum}\")
",
        "15",
    );
}

#[test]
fn list_filter_then_reduce() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list
use system.collections.transformable

let l = List([-1, 2, -3, 4, 5])
let result = l.filter(fn(x int) bool: x > 0).reduce(0, fn(a int, b int) int: a + b)
println(f\"{result}\")
",
        "11",
    );
}

#[test]
fn list_any_returns_true_when_element_matches() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list
use system.collections.transformable

let l = List([1, 2, 3])
println(f\"{l.any(fn(x int) bool: x > 2)}\")
println(f\"{l.any(fn(x int) bool: x > 10)}\")
",
        "true\nfalse",
    );
}

#[test]
fn list_all_returns_true_when_all_match() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list
use system.collections.transformable

let l = List([2, 4, 6])
println(f\"{l.all(fn(x int) bool: x % 2 == 0)}\")
println(f\"{l.all(fn(x int) bool: x > 3)}\")
",
        "true\nfalse",
    );
}

#[test]
fn list_flat_map_flattens_results() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list
use system.collections.transformable

let l = List([1, 2, 3])
let result = l.flat_map(fn(x int) [int]: List([x, x * 10]))
println(f\"{result.length()}\")
for item in result
    println(f\"{item}\")
",
        "6\n1\n10\n2\n20\n3\n30",
    );
}

#[test]
fn list_take_returns_first_n() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list
use system.collections.transformable

let l = List([10, 20, 30, 40, 50])
let t = l.take(3)
println(f\"{t.length()}\")
for item in t
    println(f\"{item}\")
",
        "3\n10\n20\n30",
    );
}

#[test]
fn list_skip_skips_first_n() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list
use system.collections.transformable

let l = List([10, 20, 30, 40, 50])
let d = l.skip(2)
println(f\"{d.length()}\")
for item in d
    println(f\"{item}\")
",
        "3\n30\n40\n50",
    );
}

#[test]
fn list_unique_deduplicates() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list
use system.collections.transformable

let l = List([1, 2, 2, 3, 1, 3])
let u = l.unique()
println(f\"{u.length()}\")
",
        "3",
    );
}

#[test]
fn list_sorted_by_comparator() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list
use system.collections.transformable

let l = List([3, 1, 4, 1, 5, 9, 2])
let s = l.sorted_by(fn(a int, b int) int: a - b)
for item in s
    println(f\"{item}\")
",
        "1\n1\n2\n3\n4\n5\n9",
    );
}

#[test]
fn list_sum_of_integers() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list
use system.collections.transformable

let l = List([1, 2, 3, 4, 5])
println(f\"{l.sum()}\")
",
        "15",
    );
}

#[test]
fn list_min_and_max() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list
use system.collections.transformable

let l = List([3, 1, 4, 1, 5, 9, 2])
println(f\"{l.min()}\")
println(f\"{l.max()}\")
",
        "1\n9",
    );
}
