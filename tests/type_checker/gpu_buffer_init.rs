// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU buffer-initializer metadata is produced during semantic analysis: a
//! `gpu let`/`gpu var` bound to a compile-time constant array/list literal (or
//! a sized `Array<T, N>()` constructor) records its initial data on the type
//! checker so the web-gpu emitter can consume it without re-walking the AST.

use crate::type_checker::utils::type_checker_result;
use miri::type_checker::GpuBufferInit;

fn buffer_inits(source: &str) -> Vec<GpuBufferInit> {
    type_checker_result(source)
        .type_checker
        .gpu_buffer_inits
        .into_values()
        .collect()
}

fn init_named<'a>(inits: &'a [GpuBufferInit], name: &str) -> Option<&'a GpuBufferInit> {
    inits.iter().find(|init| init.name == name)
}

#[test]
fn test_gpu_let_int_literal_array_is_collected() {
    let inits = buffer_inits("fn main()\n    gpu let a = [1, 2, 3, 4]\n    a.length()\n");
    let init = init_named(&inits, "a").expect("buffer init for 'a' should be present");
    assert_eq!(init.values, vec![1.0, 2.0, 3.0, 4.0]);
    assert_eq!(init.length, None);
}

#[test]
fn test_gpu_let_float_literal_array_is_collected() {
    let inits = buffer_inits("fn main()\n    gpu let a = [1.0, 2.0, 3.0]\n    a.length()\n");
    let init = init_named(&inits, "a").expect("buffer init for 'a' should be present");
    assert_eq!(init.values, vec![1.0, 2.0, 3.0]);
}

#[test]
fn test_gpu_let_sized_array_constructor_records_length_and_no_values() {
    let inits = buffer_inits(
        "use system.collections.array\n\nfn main()\n    gpu let a = Array<int, 8>()\n    a.length()\n",
    );
    let init = init_named(&inits, "a").expect("buffer init for 'a' should be present");
    assert!(init.values.is_empty());
    assert_eq!(init.length, Some(8));
}

#[test]
fn test_host_let_is_not_collected() {
    let inits = buffer_inits("fn main()\n    let a = [1, 2, 3, 4]\n    a.length()\n");
    assert!(
        init_named(&inits, "a").is_none(),
        "host (non-gpu) bindings must not produce buffer-init metadata"
    );
}

#[test]
fn test_same_named_buffers_in_two_functions_are_both_collected() {
    let inits = buffer_inits(
        "fn a()\n    gpu var buf = [1.0, 2.0]\n    buf.length()\n\nfn b()\n    gpu var buf = [3.0, 4.0, 5.0]\n    buf.length()\n",
    );
    let mut values: Vec<Vec<f64>> = inits
        .iter()
        .filter(|init| init.name == "buf")
        .map(|init| init.values.clone())
        .collect();
    values.sort_by_key(Vec::len);
    assert_eq!(values, vec![vec![1.0, 2.0], vec![3.0, 4.0, 5.0]]);
}

#[test]
fn test_gpu_let_bound_to_a_call_is_not_collected() {
    let inits = buffer_inits(
        "fn make() [int; 2]\n    return [1, 2]\n\nfn main()\n    gpu let a = make()\n    a.length()\n",
    );
    assert!(
        init_named(&inits, "a").is_none(),
        "an initializer that runs host code has no build-time contents"
    );
}

#[test]
fn test_negated_literals_are_build_time_constants() {
    let inits = buffer_inits("fn main()\n    gpu let a = [1.0, -1.0, -2.5]\n    a.length()\n");
    let init = init_named(&inits, "a").expect("buffer init for 'a' should be present");
    assert_eq!(init.values, vec![1.0, -1.0, -2.5]);
}
