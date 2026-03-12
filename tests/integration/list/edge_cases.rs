// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn list_empty_boundary_conditions() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List<int>()
println(f\"{l.is_empty()}\")
println(f\"{l.first() ?? -1}\")
println(f\"{l.last() ?? -1}\")
",
        "true\n-1\n-1",
    );
}

#[test]
fn list_capacity_reallocation_stress() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List<int>()
for i in 0..100
    l.push(i)
println(f\"{l.length()}\")
println(f\"{l[99]}\")
",
        "100\n99",
    );
}

#[test]
fn list_out_of_bounds_insert() {
    // Runtime's miri_rt_list_insert silently returns false on OOB index (no crash).
    assert_runs(
        "
use system.collections.list

let l = List([1, 2])
l.insert(5, 99)
",
    );
}

#[test]
fn list_nested_management() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let inner1 = List([1, 2])
let inner2 = List([3, 4])
let outer = List([inner1, inner2])

println(f\"{outer.length()}\")
let got_inner = outer[1]
println(f\"{got_inner[0]} {got_inner[1]}\")

outer[0].push(99)
let first_inner = outer[0]
println(f\"{first_inner[2]}\")
",
        "2\n3 4\n99",
    );
}

#[test]
fn list_insert_at_length() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([1, 2])
l.insert(2, 99)
println(f\"{l[0]} {l[1]} {l[2]}\")
println(f\"{l.length()}\")
",
        "1 2 99\n3",
    );
}
