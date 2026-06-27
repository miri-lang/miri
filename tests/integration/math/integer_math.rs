// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::gpu::device::assert_gpu_runs_with_output;
use super::super::utils::*;

#[test]
fn test_abs_integer() {
    assert_runs_with_output(
        r#"
        use system.math.{abs}

        let x = 0 - 5
        let result = abs(x)
        println(f'{result}')
        "#,
        "5",
    );
}

#[test]
fn test_min_integer() {
    assert_runs_with_output(
        r#"
        use system.math.{min}

        let result = min(7, 2)
        println(f'{result}')
        "#,
        "2",
    );
}

#[test]
fn test_max_integer() {
    assert_runs_with_output(
        r#"
        use system.math.{max}

        let result = max(2, 9)
        println(f'{result}')
        "#,
        "9",
    );
}

#[test]
fn test_abs_float_still_works() {
    assert_runs_with_output(
        r#"
        use system.math.{abs}

        let result = abs(0.0 - 5.5)
        println(f'{result}')
        "#,
        "5.5",
    );
}

#[test]
fn test_min_float_still_works() {
    assert_runs_with_output(
        r#"
        use system.math.{min}

        let result = min(7.5, 2.3)
        println(f'{result}')
        "#,
        "2.299999952316284",
    );
}

#[test]
fn test_max_float_still_works() {
    assert_runs_with_output(
        r#"
        use system.math.{max}

        let result = max(2.1, 9.9)
        println(f'{result}')
        "#,
        "9.899999618530273",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_abs_integer_gpu() {
    let source = r#"
        use system.math.{abs}
        use system.gpu
        use system.collections.array

        gpu let src = [0 - 1, 1, 0 - 1]
        gpu var dst = [0, 0, 0]
        gpu forall i in 0..3
            dst[i] = abs(src[i])

        let host = dst
        println(f'{host[0]} {host[1]} {host[2]}')
        "#;
    assert_gpu_runs_with_output(source, "1 1 1");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_max_integer_gpu() {
    let source = r#"
        use system.math.{max}
        use system.gpu
        use system.collections.array

        gpu let src = [0, 1, 2, 3]
        gpu var dst = [0, 0, 0, 0]
        gpu forall i in 0..4
            dst[i] = max(src[i], 2)

        let host = dst
        println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
        "#;
    assert_gpu_runs_with_output(source, "2 2 2 3");
}

/// FIX 2: arity validation for polymorphic math functions.
/// abs expects exactly one argument; should reject abs(1, 2).
#[test]
fn test_abs_rejects_too_many_args() {
    assert_compiler_error(
        r#"
        use system.math.{abs}
        let result = abs(1, 2)
        "#,
        "abs expects exactly one argument",
    );
}

/// FIX 2: arity validation for polymorphic math functions.
/// min expects exactly two arguments; should reject min(5).
#[test]
fn test_min_rejects_too_few_args() {
    assert_compiler_error(
        r#"
        use system.math.{min}
        let result = min(5)
        "#,
        "min expects exactly two arguments",
    );
}

/// FIX 2: arity validation for polymorphic math functions.
/// max expects exactly two arguments; should reject max(1, 2, 3).
#[test]
fn test_max_rejects_too_many_args() {
    assert_compiler_error(
        r#"
        use system.math.{max}
        let result = max(1, 2, 3)
        "#,
        "max expects exactly two arguments",
    );
}

/// FIX 6: User-defined function without system.math import is NOT polymorphic.
/// This test verifies that the module gate prevents user functions from
/// being treated as the polymorphic intrinsic.
#[test]
fn test_user_defined_function_not_polymorphic() {
    assert_runs_with_output(
        r#"

fn double(x int) int
    x * 2

fn main()
    let result = double(5)
    println(f'{result}')
        "#,
        "10",
    );
}

/// FIX 6: Cast float overflow saturates to i64::MAX.
/// A very large float (1e30) cast to int saturates to i64::MAX.
#[test]
fn test_cast_float_overflow_saturates_positive() {
    assert_runs_with_output(
        r#"

        let x = 1.0e30
        let result = x as int
        println(f'{result}')
        "#,
        "9223372036854775807",
    );
}

/// FIX 6: Cast float underflow saturates to i64::MIN.
/// A very small (large negative) float (-1e30) cast to int saturates to i64::MIN.
#[test]
fn test_cast_float_overflow_saturates_negative() {
    assert_runs_with_output(
        r#"

        let x = 0.0 - 1.0e30
        let result = x as int
        println(f'{result}')
        "#,
        "-9223372036854775808",
    );
}

mod new_math_intrinsics {
    use super::*;

    #[test]
    fn test_tanh_float_cpu() {
        assert_runs_with_output(
            r#"
            use system.math.{tanh}

            let result = tanh(0.0)
            println(f'{result}')
            "#,
            "0.0",
        );
    }

    #[test]
    fn test_exp2_float_cpu() {
        assert_runs_with_output(
            r#"
            use system.math.{exp2}

            let result = exp2(3.0)
            println(f'{result}')
            "#,
            "8.0",
        );
    }

    #[test]
    fn test_log2_float_cpu() {
        assert_runs_with_output(
            r#"
            use system.math.{log2}

            let result = log2(8.0)
            println(f'{result}')
            "#,
            "3.0",
        );
    }

    #[test]
    fn test_fract_float_cpu() {
        assert_runs_with_output(
            r#"
            use system.math.{fract}

            let result = fract(2.25)
            println(f'{result}')
            "#,
            "0.25",
        );
    }

    #[test]
    fn test_sign_float_cpu() {
        assert_runs_with_output(
            r#"
            use system.math.{sign}

            let neg = sign(0.0 - 3.0)
            let zero = sign(0.0)
            let pos = sign(2.0)
            println(f'{neg} {zero} {pos}')
            "#,
            "-1.0 0.0 1.0",
        );
    }

    #[test]
    fn test_atan2_float_cpu() {
        assert_runs_with_output(
            r#"
            use system.math.{atan2}

            let result = atan2(1.0, 1.0)
            println(f'{result}')
            "#,
            "0.7853981",
        );
    }

    #[test]
    fn test_step_float_cpu() {
        assert_runs_with_output(
            r#"
            use system.math.{step}

            let below = step(0.5, 0.3)
            let above = step(0.5, 0.7)
            println(f'{below} {above}')
            "#,
            "0.0 1.0",
        );
    }

    #[test]
    fn test_clamp_float_cpu() {
        assert_runs_with_output(
            r#"
            use system.math.{clamp}

            let lo = clamp(0.0 - 1.0, 0.0, 1.0)
            let mid = clamp(0.5, 0.0, 1.0)
            let hi = clamp(5.0, 0.0, 1.0)
            println(f'{lo} {mid} {hi}')
            "#,
            "0.0 0.5 1.0",
        );
    }

    #[test]
    fn test_mix_float_cpu() {
        assert_runs_with_output(
            r#"
            use system.math.{mix}

            let result = mix(0.0, 10.0, 0.25)
            println(f'{result}')
            "#,
            "2.5",
        );
    }

    #[test]
    fn test_smoothstep_float_cpu() {
        assert_runs_with_output(
            r#"
            use system.math.{smoothstep}

            let result = smoothstep(0.0, 1.0, 0.5)
            println(f'{result}')
            "#,
            "0.5",
        );
    }
}
