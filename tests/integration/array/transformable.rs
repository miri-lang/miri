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
fn array_take_and_drop() {
    assert_runs_with_output(
        "
use system.io
use system.collections.array
use system.collections.transformable

let a = [10, 20, 30, 40, 50]
let t = a.take(2)
let d = a.drop(3)
println(f\"{t.length()}\")
println(f\"{d.length()}\")
",
        "2\n2",
    );
}
