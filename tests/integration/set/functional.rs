// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn set_filter_keeps_matching_elements() {
    assert_runs_with_output(
        "
use system.io
use system.collections.set

let s = {1, 2, 3, 4, 5}
let evens = s.filter(fn(x int) bool: x % 2 == 0)
println(f\"{evens.length()}\")
",
        "2",
    );
}

#[test]
fn set_map_transforms_elements() {
    assert_runs_with_output(
        "
use system.io
use system.collections.set

let s = {1, 2, 3}
let doubled = s.map(fn(x int) int: x * 2)
println(f\"{doubled.length()}\")
",
        "3",
    );
}

#[test]
fn set_reduce_sums_elements() {
    assert_runs_with_output(
        "
use system.io
use system.collections.set

let s = {1, 2, 3, 4, 5}
let total = s.reduce(0, fn(acc int, x int) int: acc + x)
println(f\"{total}\")
",
        "15",
    );
}
