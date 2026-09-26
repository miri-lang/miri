// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A module function the program imports by name calls intrinsics the import
//! leaves out of the program's scope; each still lowers as the intrinsic its
//! declaration makes it, never as a call to a function of that name.

use super::super::utils::*;

#[test]
fn imported_function_calls_an_intrinsic_the_import_leaves_out() {
    assert_runs_with_output(
        r#"
use system.io
use system.math.{sigmoid, value_noise}

fn main()
    println(f"{sigmoid(0.0)}")
    println(f"{value_noise(0.0, 0.0) >= 0.0}")
"#,
        "0.5\ntrue",
    );
}
