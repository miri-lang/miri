// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn array_map_doubles_elements() {
    assert_runs_with_output(
        "
use system.io
use system.collections.array
use system.collections.transformable

let a = [1, 2, 3]
let doubled = a.map(fn(x int) int: x * 2)
for item in doubled
    println(f\"{item}\")
",
        "2\n4\n6",
    );
}

#[test]
fn array_filter_keeps_positives() {
    assert_runs_with_output(
        "
use system.io
use system.collections.array
use system.collections.transformable

let a = [-1, 2, -3, 4]
let pos = a.filter(fn(x int) bool: x > 0)
println(f\"{pos.length()}\")
",
        "2",
    );
}

#[test]
fn array_reduce_product() {
    assert_runs_with_output(
        "
use system.io
use system.collections.array
use system.collections.transformable

let a = [1, 2, 3, 4]
let product = a.reduce(1, fn(acc int, x int) int: acc * x)
println(f\"{product}\")
",
        "24",
    );
}

#[test]
fn array_any_all() {
    assert_runs_with_output(
        "
use system.io
use system.collections.array
use system.collections.transformable

let a = [2, 4, 6]
println(f\"{a.any(fn(x int) bool: x > 5)}\")
println(f\"{a.all(fn(x int) bool: x % 2 == 0)}\")
",
        "true\ntrue",
    );
}

#[test]
fn array_take_and_skip() {
    assert_runs_with_output(
        "
use system.io
use system.collections.array
use system.collections.transformable

let a = [10, 20, 30, 40, 50]
let t = a.take(2)
let d = a.skip(3)
println(f\"{t.length()}\")
println(f\"{d.length()}\")
",
        "2\n2",
    );
}

#[test]
fn array_flat_map_flattens_results() {
    assert_runs_with_output(
        "
use system.io
use system.collections.array
use system.collections.transformable
use system.collections.list

let a = [1, 2, 3]
let result = a.flat_map(fn(x int) [int]: List([x, x * 10]))
println(f\"{result.length()}\")
for item in result
    println(f\"{item}\")
",
        "6\n1\n10\n2\n20\n3\n30",
    );
}

#[test]
fn array_sorted_by_comparator() {
    assert_runs_with_output(
        "
use system.io
use system.collections.array
use system.collections.transformable

let a = [3, 1, 4, 1, 5]
let s = a.sorted_by(fn(x int, y int) int: x - y)
for item in s
    println(f\"{item}\")
",
        "1\n1\n3\n4\n5",
    );
}

#[test]
fn array_sum_of_integers() {
    assert_runs_with_output(
        "
use system.io
use system.collections.array
use system.collections.transformable

let a = [1, 2, 3, 4, 5]
println(f\"{a.sum()}\")
",
        "15",
    );
}

#[test]
fn array_min_and_max() {
    assert_runs_with_output(
        "
use system.io
use system.collections.array
use system.collections.transformable

let a = [3, 1, 4, 1, 5, 9, 2]
println(f\"{a.min()}\")
println(f\"{a.max()}\")
",
        "1\n9",
    );
}

#[test]
fn array_unique_deduplicates() {
    assert_runs_with_output(
        "
use system.io
use system.collections.array
use system.collections.transformable

let a = [1, 2, 2, 3, 1, 3]
let u = a.unique()
println(f\"{u.length()}\")
",
        "3",
    );
}

#[test]
fn array_zip_pairs_elements() {
    assert_runs_with_output(
        "
use system.io
use system.collections.array
use system.collections.list
use system.collections.transformable

let a = [1, 2, 3]
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
fn array_enumerate_indexes_elements() {
    assert_runs_with_output(
        "
use system.io
use system.collections.array
use system.collections.transformable

let a = [10, 20, 30]
let e = a.enumerate()
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
