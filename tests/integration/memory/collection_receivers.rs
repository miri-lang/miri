// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A built-in collection is a counted allocation however its type is spelled.
//! A collection constructed with its element type left to inference, and the
//! receiver of a collection method compiled once for every element type, are
//! both spelled without type arguments; each must still be retained when it is
//! handed on and released when it dies. The heap guard sees every allocation,
//! so a collection never released or released twice fails these tests.

use super::super::utils::*;

#[test]
fn a_list_built_with_its_element_type_inferred_is_released() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let xs = List([1, 2, 3])
    println(f"{xs.length()}")
"#,
        "3",
    );
}

#[test]
fn a_list_of_structs_built_with_its_element_type_inferred_is_released() {
    assert_heap_guard_output(
        r#"
use system.collections.list

struct P
    x float
    m float

fn main()
    let ps = List([P(x: 1.0, m: 2.0), P(x: 3.0, m: 4.0)])
    println(f"{ps.length()} {ps[1].m}")
"#,
        "2 4.0",
    );
}

#[test]
fn an_inferred_list_handed_through_functions_and_reassigned_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.collections.list

struct P
    x float
    m float

fn make() [P]
    List([P(x: 1.0, m: 2.0), P(x: 3.0, m: 4.0)])

fn shift(start [P]) [P]
    var ps = start
    var total = 0.0
    for p in ps
        total = total + p.x * p.m
    ps[0].x = total
    ps

fn main()
    var ps = shift(make())
    var i = 0
    while i < 3
        ps = shift(ps)
        i += 1
    println(f"{ps[0].x}")
"#,
        "196.0",
    );
}
