// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A `for` loop that binds two variables needs two names. Writing the same
//! name twice left the type checker and the generated code disagreeing about
//! which value the name meant, so a method of one ran on the other and the
//! program crashed; the loop is refused instead.

use crate::integration::utils::{assert_compiler_error, assert_runs_with_output};

const REPEATED: &str = "Loop variable 'k' is named twice";

#[test]
fn a_map_loop_naming_key_and_value_alike_is_refused() {
    assert_compiler_error(
        "
use system.collections.map

fn main()
    var m = Map<int, String>()
    m.set(7, \"x\" + \"yz\")
    for k, k in m
        println(f\"{k.length()}\")
",
        REPEATED,
    );
}

#[test]
fn a_list_loop_naming_index_and_element_alike_is_refused() {
    assert_compiler_error(
        "
use system.collections.list

fn main()
    let xs = List([\"a\", \"bc\"])
    for k, k in xs
        println(f\"{k}\")
",
        REPEATED,
    );
}

#[test]
fn a_tuple_loop_naming_both_parts_alike_is_refused() {
    assert_compiler_error(
        "
fn main()
    for k, k in [(1, \"one\"), (2, \"two\")]
        println(f\"{k}\")
",
        REPEATED,
    );
}

#[test]
fn the_refusal_carries_its_code_and_a_rename() {
    assert_compiler_error(
        "
use system.collections.map

fn main()
    var m = Map<int, String>()
    m.set(7, \"xyz\")
    for k, k in m
        println(f\"{k}\")
",
        "error[MER_TYP_064]",
    );
}

#[test]
fn distinct_names_still_bind_key_and_value() {
    assert_runs_with_output(
        "
use system.collections.map

fn main()
    var m = Map<int, String>()
    m.set(7, \"x\" + \"yz\")
    for k, v in m
        println(f\"{k} {v.length()}\")
",
        "7 3",
    );
}
