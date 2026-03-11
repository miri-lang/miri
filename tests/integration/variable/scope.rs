// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn scope_visibility() {
    assert_runs_with_output(
        r#"
use system.io

let x = 10
let result = if true
    let y = 20
    x + y
else
    0
println(f"{result}")
        "#,
        "30",
    );

    assert_runs_with_output(
        r#"
use system.io

let x = 10
if true
    let x = 20
println(f"{x}")
        "#,
        "10",
    );

    assert_compiler_error(
        r#"
if true:
    let x = 10
x
        "#,
        "Undefined variable",
    );
}
