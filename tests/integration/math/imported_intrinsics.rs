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

/// A function declared inside a body under an imported intrinsic's name
/// shadows it there: the call runs the function the name resolves to where it
/// is written, not the intrinsic of the same name.
#[test]
fn nested_function_shadows_an_imported_math_intrinsic() {
    assert_runs_with_output(
        r#"
use system.io
use system.math

fn main()
    fn floor(_x float) float
        return 42.0
    println(f"{floor(4.5)}")
"#,
        "42.0",
    );
}

/// Outside the body that shadows it, the name still reaches the intrinsic.
#[test]
fn math_intrinsic_is_called_beside_a_body_that_shadows_it() {
    assert_runs_with_output(
        r#"
use system.io
use system.math

fn shadowed() float
    fn floor(_x float) float
        return 42.0
    return floor(4.5)

fn main()
    println(f"{shadowed()} {floor(4.5)}")
"#,
        "42.0 4.0",
    );
}
