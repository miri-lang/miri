// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn gpu_array_length_reports_element_count_float() {
    assert_runs_with_output(
        "
use system.io
use system.gpu
use system.collections.array

let a = GpuArray<f32, 3>([1.0, 2.0, 3.0])
println(f\"{a.length()}\")
",
        "3",
    );
}

#[test]
fn gpu_array_length_reports_element_count_int() {
    assert_runs_with_output(
        "
use system.io
use system.gpu
use system.collections.array

let a = GpuArray<int, 4>(data: [10, 20, 30, 40])
println(f\"{a.length()}\")
",
        "4",
    );
}

#[test]
fn gpu_array_to_host_returns_original_data() {
    assert_runs_with_output(
        "
use system.io
use system.gpu
use system.collections.array

let a = GpuArray<int, 3>(data: [10, 20, 30])
let host = a.to_host()
println(f\"{host.length()}\")
for x in host
    println(f\"{x}\")
",
        "3\n10\n20\n30",
    );
}

#[test]
fn gpu_array_rejects_undefined_method() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

let a = GpuArray<int, 3>(data: [1, 2, 3])
let n = a.nonexistent()
",
        "nonexistent",
    );
}

#[test]
fn gpu_array_to_host_length_matches_input() {
    assert_runs_with_output(
        "
use system.io
use system.gpu
use system.collections.array

let a = GpuArray<f32, 4>(data: [1.5, 2.5, 3.5, 4.5])
let host = a.to_host()
println(f\"{host.length()}\")
",
        "4",
    );
}

#[test]
fn gpu_array_multiple_instantiations_in_same_scope() {
    assert_runs_with_output(
        "
use system.io
use system.gpu
use system.collections.array

let a = GpuArray<int, 2>(data: [1, 2])
let b = GpuArray<f32, 3>(data: [3.0, 4.0, 5.0])
println(f\"{a.length()} {b.length()}\")
",
        "2 3",
    );
}

#[test]
fn gpu_array_to_host_survives_source_scope() {
    assert_runs_with_output(
        "
use system.io
use system.gpu
use system.collections.array

fn make_host() Array<int, 3>
    let a = GpuArray<int, 3>(data: [7, 8, 9])
    return a.to_host()

let host = make_host()
println(f\"{host.length()}\")
for x in host
    println(f\"{x}\")
",
        "3\n7\n8\n9",
    );
}

#[test]
fn gpu_array_to_host_round_trips_float_elements() {
    // Regression: literal `[1.5, 2.5, 3.5]` is `Array<f32, 3>` (the parser
    // packs floats that round-trip in f32). Wrapping it as
    // `GpuArray<f32, 3>` keeps the element width consistent end-to-end, so
    // iteration reproduces the original values. Using `<float, 3>` /
    // `<f64, 3>` here would (correctly) be rejected by the type checker as
    // a layout-incompatible field assignment.
    assert_runs_with_output(
        "
use system.io
use system.gpu
use system.collections.array

let a = GpuArray<f32, 3>(data: [1.5, 2.5, 3.5])
let host = a.to_host()
for x in host
    println(f\"{x}\")
",
        "1.5\n2.5\n3.5",
    );
}

#[test]
fn gpu_array_to_host_round_trips_float_via_element_at() {
    // Regression: direct `to_host().element_at(idx)` path. Same root cause
    // as the iteration variant but exercises the intercepted index read
    // through a temporary rather than a for-loop.
    assert_runs_with_output(
        "
use system.io
use system.gpu
use system.collections.array

let a = GpuArray<f32, 3>(data: [1.5, 2.5, 3.5])
let host = a.to_host()
let first = host.element_at(0)
let second = host.element_at(1)
let third = host.element_at(2)
println(f\"{first} {second} {third}\")
",
        "1.5 2.5 3.5",
    );
}

#[test]
fn gpu_array_rejects_layout_incompatible_float_field() {
    // The parser packs `[1.5, ...]` as `Array<f32, 3>`. Constructing
    // `GpuArray<float, 3>` (= F64 storage) from that array would produce a
    // 4-byte-stride buffer beneath an 8-byte-stride reader. The type
    // checker refuses the mismatch before MIR lowering.
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

let a = GpuArray<float, 3>(data: [1.5, 2.5, 3.5])
",
        "Type mismatch for field 'data'",
    );
}

#[test]
fn gpu_array_rejects_size_mismatch_with_literal() {
    // `[1, 2, 3]` is `Array<int, 3>`. `GpuArray<int, 4>` declares the field
    // type as `Array<int, 4>`, so the value-generic `Size` slot now carries
    // the constraint into constructor type-checking.
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

let a = GpuArray<int, 4>(data: [1, 2, 3])
",
        "Type mismatch for field 'data'",
    );
}
