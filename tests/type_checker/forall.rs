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

#[test]
fn test_cpu_forall_2d_type_checks() {
    type_checker_test(
        r#"
fn main()
    let a = [1, 2, 3]
    forall x, y in 0..2, 0..2
        let val = a[0]
"#,
    );
}

#[test]
fn test_cpu_forall_3d_type_checks() {
    type_checker_test(
        r#"
fn main()
    let a = [1, 2, 3]
    forall x, y, z in 0..2, 0..2, 0..2
        let val = a[0]
"#,
    );
}

#[test]
fn test_bare_forall_with_gpu_capture_only_in_match_arm_resolves_to_gpu() {
    type_checker_test(
        r#"
fn main()
    gpu let g = [1, 2, 3]
    let h = 5
    forall i in 0..3
        let _ = match i
            0: g.length()
            _: h
"#,
    );
}

#[test]
fn test_bare_forall_with_gpu_capture_in_formatted_string_part_resolves_to_gpu() {
    // Tests that capture collection walks into FormattedString parts.
    // Without the FormattedString walker, 'g' would not be captured and
    // forall would mis-resolve to CPU, triggering cross-residency error.
    type_checker_error_test(
        r#"
fn main()
    gpu let g = [1, 2, 3]
    forall i in 0..3
        let len = f"{g[i]}"
"#,
        "Variable 'len' has type 'String' which is not GPU-compatible",
    );
}

#[test]
fn test_forall_captures_gpu_buffer_in_method_call_resolves_to_gpu() {
    type_checker_test(
        r#"
fn main()
    gpu let g = [1, 2, 3]
    forall i in 0..3
        let len = g.length()
"#,
    );
}
