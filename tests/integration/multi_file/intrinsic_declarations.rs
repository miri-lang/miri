// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A call lowers to an intrinsic because its callee is declared `intrinsic`,
//! not because of the module that declares it: a user module's plain function
//! named like an intrinsic stays a call, and a user module's `intrinsic fn`
//! lowers exactly as the standard library's does.

use super::utils::*;

const PLAIN_SQRT: &str = "fn sqrt(x float) float\n    return 42.0\n";
const INTRINSIC_FRACT: &str = "public intrinsic fn fract(x float) float\n";

fn project_calling<'a>(main: &'a str, module: &'a str) -> [(&'a str, &'a str); 2] {
    [("main.mi", main), ("utils/fastmath.mi", module)]
}

#[test]
fn plain_function_named_like_a_math_intrinsic_is_called_through_an_alias() {
    assert_project_runs_with_output(
        &project_calling(
            concat!(
                "use system.io\n",
                "use local.utils.fastmath as F\n",
                "println(f\"{F.sqrt(4.0) == 42.0}\")\n",
            ),
            PLAIN_SQRT,
        ),
        "true",
    );
}

#[test]
fn plain_function_named_like_a_math_intrinsic_is_called_through_an_import() {
    assert_project_runs_with_output(
        &project_calling(
            concat!(
                "use system.io\n",
                "use local.utils.fastmath\n",
                "println(f\"{sqrt(4.0) == 42.0}\")\n",
            ),
            PLAIN_SQRT,
        ),
        "true",
    );
}

#[test]
fn user_declared_math_intrinsic_lowers_through_an_alias() {
    assert_project_runs_with_output(
        &project_calling(
            concat!(
                "use system.io\n",
                "use local.utils.fastmath as F\n",
                "println(f\"{F.fract(2.25) == 0.25}\")\n",
            ),
            INTRINSIC_FRACT,
        ),
        "true",
    );
}

#[test]
fn user_declared_math_intrinsic_lowers_through_an_import() {
    assert_project_runs_with_output(
        &project_calling(
            concat!(
                "use system.io\n",
                "use local.utils.fastmath\n",
                "println(f\"{fract(2.25) == 0.25}\")\n",
            ),
            INTRINSIC_FRACT,
        ),
        "true",
    );
}

#[test]
fn plain_function_named_like_an_assertion_is_called() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.checks\n",
                    "println(f\"{assert_eq(1, 2)}\")\n",
                ),
            ),
            (
                "utils/checks.mi",
                "fn assert_eq(a int, b int) bool\n    return a == b\n",
            ),
        ],
        "false",
    );
}

#[test]
fn user_declared_assertion_intrinsic_lowers_to_the_assertion() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.utils.checks\n",
                    "assert(1 + 1 == 2)\n",
                    "println(\"ok\")\n",
                ),
            ),
            (
                "utils/checks.mi",
                "public intrinsic fn assert(condition bool, message String = \"\")\n",
            ),
        ],
        "ok",
    );
}

const INTRINSIC_ABS_MIN_MAX: &str = concat!(
    "public intrinsic fn abs(x float) float\n",
    "public intrinsic fn min(a float, b float) float\n",
    "public intrinsic fn max(a float, b float) float\n",
);

#[test]
fn user_declared_polymorphic_math_intrinsic_types_at_its_argument() {
    assert_project_runs_with_output(
        &project_calling(
            concat!(
                "use system.io\n",
                "use local.utils.fastmath\n",
                "let a int = abs(-3)\n",
                "let b int = min(2, 5)\n",
                "let c int = max(2, 5)\n",
                "println(f\"{a} {b} {c}\")\n",
            ),
            INTRINSIC_ABS_MIN_MAX,
        ),
        "3 2 5",
    );
}

#[test]
fn user_declared_polymorphic_math_intrinsic_types_at_its_argument_through_an_alias() {
    assert_project_runs_with_output(
        &project_calling(
            concat!(
                "use system.io\n",
                "use local.utils.fastmath as F\n",
                "let a int = F.abs(-3)\n",
                "println(f\"{a}\")\n",
            ),
            INTRINSIC_ABS_MIN_MAX,
        ),
        "3",
    );
}

#[test]
fn plain_function_named_abs_keeps_its_declared_signature() {
    assert_project_compiler_error(
        &project_calling(
            concat!("use local.utils.fastmath\n", "let a int = abs(-3)\n",),
            "public fn abs(x float) float\n    return x\n",
        ),
        "Type mismatch",
    );
}
