// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A collection literal mixing bare values and `None`, written where an
//! optional element type is declared, holds that optional: each bare value is
//! stored as `Some`.

use super::utils::*;

#[test]
fn an_array_declared_optional_lifts_its_bare_elements() {
    assert_runs_with_output(
        r#"
fn main()
    let xs [int?; 3] = [1, None, 3]
    println(f"{xs[0]} {xs[1]} {xs[2]}")
"#,
        "Some(1) None Some(3)",
    );
}

#[test]
fn a_list_constructor_naming_an_optional_lifts_its_bare_elements() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let xs = List<int?>([None, 2, 3])
    println(f"{xs[0]} {xs[1]} {xs.length()}")
"#,
        "None Some(2) 3",
    );
}

#[test]
fn a_list_binding_declared_optional_lifts_its_bare_strings() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let xs [String?] = List(["a" + "b", None])
    println(f"{xs[0]} {xs[1]}")
"#,
        "Some(ab) None",
    );
}

#[test]
fn an_undeclared_literal_mixing_none_and_values_is_still_refused() {
    assert_compiler_error(
        r#"
fn main()
    let xs = [1, None, 3]
    println(f"{xs[0]}")
"#,
        "Array elements must have the same type",
    );
}
