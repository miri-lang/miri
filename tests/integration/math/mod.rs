// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

pub mod integer_math;

use super::utils::*;

#[test]
fn test_math_constants() {
    assert_runs_with_output(
        r#"
        use system.math as M
        use system.io

        print(f"{M.PI}\n")
        print(f"{M.E}\n")
        "#,
        "3.141592653589793\n2.718281828459045",
    );
}

#[test]
fn test_math_functions_basic() {
    assert_runs_with_output(
        r#"
        use system.math.{sqrt, abs, sin, cos, pow, min, max}
        use system.io

        print(f"{sqrt(16.0)}\n")
        print(f"{abs(-5.0)}\n")
        print(f"{pow(2.0, 3.0)}\n")
        print(f"{min(10.0, 20.0)}\n")
        print(f"{max(10.0, 20.0)}\n")
        "#,
        "4.0\n5.0\n8.0\n10.0\n20.0",
    );
}

#[test]
fn test_math_functions_trig() {
    // Note: Trig functions might have small precision differences, but 0.0 should be exact.
    assert_runs_with_output(
        r#"
        use system.math.{sin, cos, tan}
        use system.io

        print(f"{sin(0.0)}\n")
        print(f"{cos(0.0)}\n")
        print(f"{tan(0.0)}\n")
        "#,
        "0.0\n1.0\n0.0",
    );
}

#[test]
fn test_math_functions_rounding() {
    assert_runs_with_output(
        r#"
        use system.math.{floor, ceil, round}
        use system.io

        print(f"{floor(1.9)}\n")
        print(f"{ceil(1.1)}\n")
        print(f"{round(1.5)}\n")
        print(f"{round(2.5)}\n")
        "#,
        "1.0\n2.0\n2.0\n2.0",
    );
}

#[test]
fn test_math_functions_log_exp() {
    assert_runs_with_output(
        r#"
        use system.math as M
        use system.math.{log, exp}
        use system.io

        print(f"{log(M.E)}\n")
        print(f"{exp(0.0)}\n")
        "#,
        "1.0\n1.0",
    );
}

#[test]
fn test_math_inf_constant() {
    assert_runs_with_output(
        r#"
        use system.math as M
        use system.io

        print(f"{M.INF}\n")
        "#,
        "inf",
    );
}

#[test]
fn test_math_sigmoid_cpu() {
    let source = r#"
use system.math
use system.io

print(f"{sigmoid(0.0)}\n")
print(f"{sigmoid(1.0)}\n")
print(f"{sigmoid(-1.0)}\n")
print(f"{sigmoid(2.0)}\n")
"#;
    // sigmoid(0.0) = 0.5
    // sigmoid(1.0) ≈ 0.7310585786
    // sigmoid(-1.0) ≈ 0.2689414214
    // sigmoid(2.0) ≈ 0.8807970718
    assert_runs(source);
}

#[test]
fn test_math_tanh_cpu() {
    let source = r#"
use system.math
use system.io

print(f"{tanh(0.0)}\n")
print(f"{tanh(1.0)}\n")
print(f"{tanh(-1.0)}\n")
"#;
    // tanh(0.0) = 0.0
    // tanh(1.0) ≈ 0.7615941559
    // tanh(-1.0) ≈ -0.7615941559
    assert_runs(source);
}
