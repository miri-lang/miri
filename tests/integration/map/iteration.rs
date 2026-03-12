// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_for_loop_custom_type() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

let items = Map<int, int>()
for i in 0..10
    items.set(i, i * 100)

for k, v in items
    println(f"{k} = {v}")
"#,
        "5 = 500\n4 = 400\n7 = 700\n6 = 600\n1 = 100\n0 = 0\n3 = 300\n2 = 200\n9 = 900\n8 = 800\n",
    );
}

#[test]
fn map_for_loop_keys() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

let m = {1: 10, 2: 20, 3: 30}
var sum = 0
for k in m
    sum = sum + k
println(f"{sum}")
"#,
        "6",
    );
}

#[test]
fn map_for_loop_keys_and_values() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

let m = {1: 10, 2: 20, 3: 30}
var key_sum = 0
var val_sum = 0
for k, v in m
    key_sum = key_sum + k
    val_sum = val_sum + v
println(f"{key_sum}")
println(f"{val_sum}")
"#,
        "6\n60",
    );
}

#[test]
fn map_iterate_empty_instantiated() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

let m = Map<int, int>()
var ran = false
for k, v in m
    ran = true

println(f"{ran}")
"#,
        "false",
    );
}
