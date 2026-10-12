// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `x in a` on an array is the call `a.contains(x)`.

use super::utils::*;

#[test]
fn membership_in_an_array_literal_answers_by_value() {
    assert_runs_with_output(
        r#"
fn main()
    let a = [1, 2, 3]
    println(f"{2 in a} {7 in a}")
"#,
        "true false",
    );
}

#[test]
fn membership_refuses_an_element_of_another_type() {
    assert_compiler_error(
        r#"
fn main()
    let a = [1, 2, 3]
    println(f"{'x' in a}")
"#,
        "cannot check membership of String in collection of int",
    );
}
