// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn mutability_checks() {
    assert_runs_with_output(
        r#"
use system.io

var x = 10
x = 20
println(f"{x}")
        "#,
        "20",
    );

    assert_compiler_error(
        r#"
let x = 10
x = 20
        "#,
        "Cannot assign to immutable variable 'x'",
    );
}
