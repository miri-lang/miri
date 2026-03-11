// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_simple_enum() {
    assert_runs(
        r#"
enum Status
    Ok
    Error

fn main()
    let s = Status.Ok
    "#,
    );
}

#[test]
fn test_enum_with_data() {
    assert_runs(
        r#"
enum Result
    Success(int)
    Failure(String)

fn main()
    let r = Result.Success(42)
    "#,
    );
}
