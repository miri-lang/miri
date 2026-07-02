// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// `Tensor<T, Rank>` is a stdlib value-generic container: `Rank` is a
// compile-time value generic (static rank), while the extents are runtime
// values carried in `shape: Array<int, Rank>` (dynamic dimensions). It is added
// purely as an `.mi` edit implementing `Accelerable` — the compiler carries no
// `Tensor`-specific logic. These tests exercise the value-generic mechanics, the
// `Accelerable` capability, and the constructor rank check.

use super::utils::*;

#[test]
fn tensor_reports_static_rank_and_runtime_dimensions() {
    assert_runs_with_output(
        "
use system.collections.tensor
use system.collections.list

let t = Tensor<int, 2>(shape: [2, 3], data: List([1, 2, 3, 4, 5, 6]))
println(f\"{t.rank()}\")
println(f\"{t.dimension(0)}\")
println(f\"{t.dimension(1)}\")
",
        "2\n2\n3",
    );
}

#[test]
fn tensor_size_is_the_product_of_dimensions() {
    assert_runs_with_output(
        "
use system.collections.tensor
use system.collections.list

let t = Tensor<int, 3>(shape: [2, 3, 4], data: List([0]))
println(f\"{t.size()}\")
",
        "24",
    );
}

#[test]
fn tensor_flat_element_read_threads_the_element_type() {
    assert_runs_with_output(
        "
use system.collections.tensor
use system.collections.list

let t = Tensor<int, 1>(shape: [3], data: List([10, 20, 30]))
println(f\"{t.element_at(2)}\")
",
        "30",
    );
}

// The headline property: `Tensor` is gpu-eligible purely because it declares
// `implements Accelerable`, with a non-int element type threaded through `T`.
#[test]
fn tensor_of_floats_is_gpu_accelerable() {
    assert_type_checks(
        "
use system.collections.tensor
use system.collections.list

fn main()
    gpu let t = Tensor<f32, 1>(shape: [3], data: List([1.5, 2.5, 3.5]))
",
    );
}

// The value-generic `Rank` threads into constructor field type-checking: a
// two-element shape literal is `Array<int, 2>` and cannot satisfy a `Rank`-3
// tensor's `shape: Array<int, 3>` field.
#[test]
fn tensor_rejects_shape_literal_of_wrong_rank() {
    assert_compiler_error(
        "
use system.collections.tensor
use system.collections.list

let t = Tensor<int, 3>(shape: [2, 3], data: List([1, 2, 3, 4, 5, 6]))
",
        "Type mismatch for field 'shape'",
    );
}
