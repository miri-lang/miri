// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An optional written `Option<T>` is `T?` wherever a type is written,
//! including the parameter and return types of a lambda, which its function
//! type carries to every later stage.

use super::super::utils::*;

#[test]
fn test_a_lambda_returning_an_optional_written_in_full_returns_it() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let a Option<int> = Some(4)
    let c = fn() Option<int>: a
    match c()
        Some(v): println(f"Some:{v}")
        None: println("None")
"#,
        "Some:4",
    );
}

#[test]
fn test_a_lambda_taking_an_optional_written_in_full_takes_it() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let k = fn(x Option<String>) String: x ?? "none"
    println(k(Some("given")))
    println(k(None))
"#,
        "given\nnone",
    );
}
