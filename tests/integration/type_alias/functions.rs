// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn type_alias_as_function_parameter_and_return() {
    assert_runs_with_output(
        r#"
use system.io
type MyInt is int

fn double(x MyInt) MyInt
    return x * 2

println(f"{double(21)}")
"#,
        "42",
    );
}
