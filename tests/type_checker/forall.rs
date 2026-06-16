// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{type_checker_error_test, type_checker_test};

#[test]
fn test_cpu_only_data_resolves_to_cpu() {
    type_checker_test(
        r#"
fn main()
    let arr = [1, 2, 3]
    forall i in 0..3
        let x = arr[i]
"#,
    );
}

#[test]
fn test_gpu_only_data_resolves_to_gpu() {
    type_checker_test(
        r#"
fn main()
    gpu let g = [1, 2, 3]
    forall i in 0..3
        let x = g[i]
"#,
    );
}

#[test]
fn test_cpu_and_gpu_data_resolves_to_gpu() {
    type_checker_test(
        r#"
fn main()
    let h = 5
    gpu let g = [1, 2, 3]
    forall i in 0..3
        let x = g[i] + h
"#,
    );
}

#[test]
fn test_gpu_forall_with_no_gpu_data_rejects() {
    type_checker_error_test(
        r#"
fn main()
    let arr = [1, 2, 3]
    gpu forall i in 0..3
        let x = arr[i]
"#,
        "'gpu forall' requires at least one gpu-resident buffer; none found (annotate data with 'gpu let')",
    );
}

#[test]
fn test_gpu_forall_empty_body_rejects() {
    type_checker_error_test(
        r#"
fn main()
    gpu forall i in 0..3
        let x = 5
"#,
        "'gpu forall' requires at least one gpu-resident buffer",
    );
}

#[test]
fn test_reduction_with_outer_scalar_accumulator_rejects() {
    type_checker_error_test(
        r#"
fn main()
    var total = 0
    gpu let g = [1, 2, 3]
    gpu forall i in 0..3
        total = total + g[i]
"#,
        "loop-carried accumulator 'total' makes 'forall' iterations order-dependent",
    );
}

#[test]
fn test_local_accumulator_inside_body_does_not_reject() {
    type_checker_test(
        r#"
fn main()
    gpu let g = [1, 2, 3]
    gpu forall i in 0..3
        var acc = 0
        acc = acc + g[i]
"#,
    );
}

#[test]
fn test_1d_gpu_forall_type_checks() {
    type_checker_test(
        r#"
fn main()
    gpu let g = [1, 2, 3]
    gpu forall i in 0..3
        let x = g[i]
"#,
    );
}

#[test]
fn test_2d_gpu_forall_type_checks() {
    type_checker_test(
        r#"
fn main()
    gpu let g = [1, 2, 3, 4]
    gpu forall x, y in 0..2, 0..2
        let val = g[0]
"#,
    );
}

#[test]
fn test_3d_gpu_forall_type_checks() {
    type_checker_test(
        r#"
fn main()
    gpu let g = [1, 2, 3]
    gpu forall x, y, z in 0..2, 0..2, 0..2
        let val = g[0]
"#,
    );
}
