// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A GPU program the compiler refuses is told so under the refusal's own code,
//! with what it found, what belongs there and how to fix it — never as an
//! internal compiler error asking the author to report their own mistake.

use crate::integration::utils::assert_compiler_error;
use crate::utils::miri_check;

const INTERNAL_ERROR: &str = "MER_CG_008";

/// Checks `source` and returns everything the check printed, asserting it
/// refused the program under `code` and did not call it an internal error.
fn refusal_of(source: &str, code: &str) -> String {
    assert_compiler_error(source, &format!("[{code}]"));
    let output = miri_check(source).output();
    assert!(
        !output.contains(INTERNAL_ERROR) && !output.contains("Please report this"),
        "a refusal of the program must not read as an internal error:\n{output}"
    );
    output
}

fn assert_mentions(output: &str, fragments: &[&str]) {
    for fragment in fragments {
        assert!(
            output.contains(fragment),
            "expected `{fragment}` in:\n{output}"
        );
    }
}

#[test]
fn atomic_on_a_float_buffer_names_the_buffer_type_and_the_atomic_one() {
    let output = refusal_of(
        "
use system.collections.array
use system.gpu.atomic

fn main()
    gpu var hist = Array<f32, 4>()
    gpu forall i in 0..4
        atomic_add(hist, i, 1.0)
",
        "MER_TYP_029",
    );
    assert_mentions(
        &output,
        &[
            "`atomic_add` needs a buffer of `Array<Atomic<i32>, N>` or `Array<Atomic<u32>, N>`",
            "`hist` is `Array<f32, 4>`",
            "declare `hist` as `Array<Atomic<i32>, 4>`",
        ],
    );
}

#[test]
fn atomic_on_a_plain_u32_buffer_suggests_the_u32_atomic() {
    let output = refusal_of(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var plain_buf = Array<u32, 4>()
    gpu forall i in 0..4
        atomic_add(plain_buf, i, 1 as u32)
",
        "MER_TYP_029",
    );
    assert_mentions(
        &output,
        &[
            "`plain_buf` is `Array<u32, 4>`",
            "declare `plain_buf` as `Array<Atomic<u32>, 4>`",
        ],
    );
}

#[test]
fn launch_block_size_from_a_variable_names_the_variable() {
    let output = refusal_of(
        "
use system.gpu

gpu fn my_kernel()
    let x = 1

fn main()
    var block_size = 16
    my_kernel().launch(Dim3(1, 1, 1), Dim3(block_size, 16, 1))
",
        "MER_TYP_022",
    );
    assert_mentions(
        &output,
        &[
            "`block_size` is not an integer literal",
            "e.g. `Dim3(16, 16, 1)`",
        ],
    );
}

#[test]
fn launch_block_dimension_of_zero_names_the_dimension() {
    let output = refusal_of(
        "
use system.gpu

gpu fn my_kernel()
    let x = 1

fn main()
    my_kernel().launch(Dim3(1, 1, 1), Dim3(16, 0, 1))
",
        "MER_TYP_019",
    );
    assert_mentions(&output, &["`Dim3(16, 0, 1)`", "at least 1"]);
}

#[test]
fn launch_block_dimension_below_zero_is_not_called_a_non_literal() {
    let output = refusal_of(
        "
use system.gpu

gpu fn my_kernel()
    let x = 1

fn main()
    my_kernel().launch(Dim3(1, 1, 1), Dim3(16, -2, 1))
",
        "MER_TYP_019",
    );
    assert_mentions(&output, &["at least 1"]);
}

#[test]
fn host_array_passed_to_a_gpu_fn_names_the_binding_and_the_fix() {
    let output = refusal_of(
        "
use system.collections.array

gpu fn my_kernel(a Array<f32,4>)
    let x = 1

fn main()
    let a Array<f32,4> = [1.0, 2.0, 3.0, 4.0]
    my_kernel(a).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
",
        "MER_TYP_021",
    );
    assert_mentions(&output, &["`a` is host-resident", "`gpu let a = ...`"]);
}

#[test]
fn array_built_in_place_for_a_gpu_fn_is_told_to_bind_it_first() {
    let output = refusal_of(
        "
use system.collections.array

gpu fn my_kernel(a Array<f32,4>)
    let x = 1

fn main()
    my_kernel([1.0, 2.0, 3.0, 4.0]).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
",
        "MER_TYP_021",
    );
    assert_mentions(&output, &["host-resident", "gpu let"]);
}

/// A launch mistake is reported by the type checker, in the same batch as an
/// ordinary type error elsewhere in the program, not only once that is fixed.
#[test]
fn a_launch_block_mistake_is_reported_beside_a_type_error() {
    let output = miri_check(
        "
use system.gpu

gpu fn my_kernel()
    let x = 1

fn main()
    let wrong int = \"text\"
    var block_size = 16
    my_kernel().launch(Dim3(1, 1, 1), Dim3(block_size, 16, 1))
",
    )
    .output();
    assert_mentions(&output, &["MER_TYP_002", "MER_TYP_022"]);
}

#[test]
fn a_host_buffer_mistake_is_reported_beside_a_type_error() {
    let output = miri_check(
        "
use system.collections.array

gpu fn my_kernel(a Array<f32,4>)
    let x = 1

fn main()
    let wrong int = \"text\"
    let a Array<f32,4> = [1.0, 2.0, 3.0, 4.0]
    my_kernel(a).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
",
    )
    .output();
    assert_mentions(
        &output,
        &["MER_TYP_002", "MER_TYP_021", "`a` is host-resident"],
    );
}

/// A device buffer passed to a `gpu fn` is accepted.
#[test]
fn a_device_buffer_passed_to_a_gpu_fn_is_accepted() {
    let result = miri_check(
        "
use system.gpu
use system.collections.array

gpu fn my_kernel(a Array<f32,4>)
    let x = 1

fn main()
    gpu let a Array<f32,4> = [1.0, 2.0, 3.0, 4.0]
    my_kernel(a).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
",
    );
    assert!(result.success, "{}", result.output());
}
