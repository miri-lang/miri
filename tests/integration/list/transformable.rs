// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn list_map_doubles_elements() {
    assert_runs_with_output(
        "
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
use system.collections.list
use system.collections.transformable

let l = List([1, 2, 3, 4, 5])
println(f\"{l.sum() ?? 0}\")
",
        "15",
    );
}

#[test]
fn list_min_and_max() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

let l = List([3, 1, 4, 1, 5, 9, 2])
println(f\"{l.min() ?? 0}\")
println(f\"{l.max() ?? 0}\")
",
        "1\n9",
    );
}

#[test]
fn list_zip_pairs_elements() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

let a = List([1, 2, 3])
let b = List([10, 20, 30])
let z = a.zip(b)
println(f\"{z.length()}\")
var i = 0
while i < z.length()
    let p = z[i]
    println(f\"{p.0}, {p.1}\")
    i += 1
",
        "3\n1, 10\n2, 20\n3, 30",
    );
}

#[test]
fn list_zip_truncates_to_shorter() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

let a = List([1, 2, 3, 4, 5])
let b = List([10, 20])
let z = a.zip(b)
println(f\"{z.length()}\")
var i = 0
while i < z.length()
    let p = z[i]
    println(f\"{p.0}, {p.1}\")
    i += 1
",
        "2\n1, 10\n2, 20",
    );
}

#[test]
fn list_zip_empty_yields_empty() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

let a = List<int>()
let b = List([1, 2, 3])
let z = a.zip(b)
println(f\"{z.length()}\")
",
        "0",
    );
}

#[test]
fn list_enumerate_indexes_elements() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

let l = List([10, 20, 30])
let e = l.enumerate()
println(f\"{e.length()}\")
var i = 0
while i < e.length()
    let p = e[i]
    println(f\"{p.0}, {p.1}\")
    i += 1
",
        "3\n0, 10\n1, 20\n2, 30",
    );
}

#[test]
fn list_enumerate_empty_yields_empty() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

let l = List<int>()
let e = l.enumerate()
println(f\"{e.length()}\")
",
        "0",
    );
}

#[test]
fn list_zip_for_loop_iterates_tuples() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

let a = List([1, 2, 3])
let b = List([10, 20, 30])
for p in a.zip(b)
    println(f\"{p.0}, {p.1}\")
",
        "1, 10\n2, 20\n3, 30",
    );
}

#[test]
fn list_enumerate_for_loop_iterates_tuples() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

let l = List([10, 20, 30])
for p in l.enumerate()
    println(f\"{p.0}, {p.1}\")
",
        "0, 10\n1, 20\n2, 30",
    );
}

#[test]
fn list_sum_on_empty_returns_none() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

let l = List<int>()
println(f\"{l.sum() ?? -1}\")
",
        "-1",
    );
}

#[test]
fn list_min_on_empty_returns_none() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

let l = List<int>()
println(f\"{l.min() ?? -1}\")
",
        "-1",
    );
}

#[test]
fn list_max_on_empty_returns_none() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

let l = List<int>()
println(f\"{l.max() ?? -1}\")
",
        "-1",
    );
}

#[test]
fn list_sum_min_max_single_element() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

let l = List([42])
println(f\"{l.sum() ?? 0}\")
println(f\"{l.min() ?? 0}\")
println(f\"{l.max() ?? 0}\")
",
        "42\n42\n42",
    );
}

#[test]
fn list_multiline_filter_then_map_chain() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

fn main()
    let xs = List([1, 2, 3, 4, 5])
    let squared_evens = xs
        .filter(fn(x int) bool: x % 2 == 0)
        .map(fn(x int) int: x * x)
    println(f\"{squared_evens.sum() ?? 0}\")
",
        "20",
    );
}

#[test]
fn list_multiline_chain_inside_function_body() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

fn sum_squared_evens(xs [int]) int
    xs
        .filter(fn(x int) bool: x % 2 == 0)
        .map(fn(x int) int: x * x)
        .reduce(0, fn(a int, b int) int: a + b)

fn main()
    println(f\"{sum_squared_evens(List([1, 2, 3, 4, 5]))}\")
",
        "20",
    );
}

#[test]
fn list_multiline_chain_at_top_level() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

let result = List([1, 2, 3, 4, 5])
    .filter(fn(x int) bool: x > 2)
    .map(fn(x int) int: x + 10)
println(f\"{result.length()}\")
for item in result
    println(f\"{item}\")
",
        "3\n13\n14\n15",
    );
}

#[test]
fn list_multiline_chain_with_many_steps() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

fn main()
    let r = List([1, 2, 3, 4, 5, 6, 7, 8, 9, 10])
        .filter(fn(x int) bool: x % 2 == 0)
        .map(fn(x int) int: x * 3)
        .take(3)
        .reduce(0, fn(a int, b int) int: a + b)
    println(f\"{r}\")
",
        "36",
    );
}

#[test]
fn list_multiline_chain_with_blank_line_breaks_chain() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.transformable

fn main()
    let xs = List([1, 2, 3, 4, 5])
        .filter(fn(x int) bool: x > 2)

    let total = xs.reduce(0, fn(a int, b int) int: a + b)
    println(f\"{total}\")
",
        "12",
    );
}
