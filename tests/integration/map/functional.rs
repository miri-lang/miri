// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_filter_by_value() {
    assert_runs_with_output(
        "
use system.io
use system.collections.map

let m = Map({\"a\": 1, \"b\": 2, \"c\": 3})
let filtered = m.filter(fn(k String, v int) bool: v > 1)
println(f\"{filtered.length()}\")
",
        "2",
    );
}

#[test]
fn map_map_transforms_values() {
    assert_runs_with_output(
        "
use system.io
use system.collections.map

let m = Map({\"a\": 1, \"b\": 2, \"c\": 3})
let doubled = m.map(fn(k String, v int) int: v * 2)
let total = doubled.reduce(0, fn(acc int, k String, v int) int: acc + v)
println(f\"{total}\")
",
        "12",
    );
}

#[test]
fn map_reduce_sums_values() {
    assert_runs_with_output(
        "
use system.io
use system.collections.map

let m = Map({\"a\": 1, \"b\": 2, \"c\": 3})
let total = m.reduce(0, fn(acc int, k String, v int) int: acc + v)
println(f\"{total}\")
",
        "6",
    );
}
