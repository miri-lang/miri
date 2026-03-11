// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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
