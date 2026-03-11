// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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

#[test]
fn list_reference_semantics_mutation() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l1 = List([10, 20, 30])
let l2 = l1
l1.push(40)
println(f\"{l2.length()}\")
println(f\"{l2[3]}\")
",
        "4\n40",
    );
}
