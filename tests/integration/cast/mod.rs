// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_cast_int_to_float() {
    assert_runs_with_output(
        r#"
        use system.io
        let x = 3 as float
        let result = x * 0.25
        println(f'{result}')
        "#,
        "0.75",
    );
}

#[test]
fn test_cast_float_to_int() {
    assert_runs_with_output(
        r#"
        use system.math
        use system.io
        let y = floor(3.9) as int
        println(f'{y}')
        "#,
        "3",
    );
}

#[test]
fn test_cast_float_to_int_truncates_toward_zero() {
    assert_runs_with_output(
        r#"
        use system.io
        let x = 2.7 as int
        println(f'{x}')
        "#,
        "2",
    );
}

#[test]
fn test_cast_precedence_multiply() {
    assert_runs_with_output(
        r#"
        use system.io
        let i = 4
        let result = i as float * 0.25
        println(f'{result}')
        "#,
        "1.0",
    );
}

#[test]
fn test_cast_precedence_subtract() {
    assert_runs_with_output(
        r#"
        use system.io
        let i = 5
        let result = (i as float) - (1 as float)
        println(f'{result}')
        "#,
        "4.0",
    );
}

#[test]
fn test_cast_int_to_float_roundtrip() {
    assert_runs_with_output(
        r#"
        use system.io
        let x = 42
        let fx = x as float
        let ix = fx as int
        println(f'{ix}')
        "#,
        "42",
    );
}

#[test]
fn test_cast_i32_to_float() {
    assert_runs_with_output(
        r#"
        use system.io
        let x = 300 as i32
        let fx = x as float
        println(f'{fx}')
        "#,
        "300.0",
    );
}

#[test]
fn test_cast_rejects_string() {
    assert_compiler_error(
        r#"
        use system.io
        let x = "hello" as int
        "#,
        "cast",
    );
}

#[test]
fn test_cast_rejects_list() {
    assert_compiler_error(
        r#"
        use system.io
        let x = [1, 2] as int
        "#,
        "cast",
    );
}

#[test]
fn test_cast_rejects_bool() {
    assert_compiler_error(
        r#"
        use system.io
        let x = true as float
        "#,
        "cast",
    );
}

// NOTE: GPU cast expressions in kernels are not tested yet. The implementation
// requires careful investigation of GPU capture and binding handling with cast expressions.
// #[test]
// fn test_cast_in_gpu_kernel() {
//     let source = r#"
//         use system.gpu
//         use system.collections.array
//
//         gpu var posx = [0.0, 0.0, 0.0, 0.0]
//         gpu for i in 0..4
//             posx[i] = i as float * 0.25
//
//         let host = posx
//         println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
//         "#;
//     assert_gpu_runs_with_output(source, "0.0 0.25 0.5 0.75");
// }

// NOTE: GPU cast expressions in kernels are not tested yet. The implementation
// requires careful investigation of GPU capture and binding handling with cast expressions.
// #[test]
// fn test_cast_float_to_int_in_gpu_kernel() {
//     let source = r#"
//         use system.math
//         use system.gpu
//         use system.collections.array
//
//         gpu let src = [2.1, 3.5, 4.9]
//         gpu var dst = [0, 0, 0]
//         gpu for i in 0..3
//             dst[i] = floor(src[i]) as int
//
//         let host = dst
//         println(f'{host[0]} {host[1]} {host[2]}')
//         "#;
//     assert_gpu_runs_with_output(source, "2 3 4");
// }
