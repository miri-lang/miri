// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU buffer-initializer metadata is produced during semantic analysis: a
//! `gpu let`/`gpu var` bound to a compile-time constant array/list literal (or
//! a sized `Array<T, N>()` constructor) records its initial data on the type
//! checker so the web-gpu emitter can consume it without re-walking the AST.

use miri::pipeline::Pipeline;

fn buffer_inits(
    source: &str,
) -> std::collections::HashMap<String, miri::type_checker::GpuBufferInit> {
    let pipeline = Pipeline::new();
    pipeline
        .frontend(source)
        .expect("type check should succeed")
        .type_checker
        .gpu_buffer_inits
}

#[test]
fn test_gpu_let_int_literal_array_is_collected() {
    let inits = buffer_inits("fn main()\n    gpu let a = [1, 2, 3, 4]\n    a.length()\n");
    let init = inits
        .get("a")
        .expect("buffer init for 'a' should be present");
    assert_eq!(init.values, vec![1.0, 2.0, 3.0, 4.0]);
    assert_eq!(init.elem_type, "i32");
    assert_eq!(init.length, None);
}

#[test]
fn test_gpu_let_float_literal_array_records_f32_element() {
    let inits = buffer_inits("fn main()\n    gpu let a = [1.0, 2.0, 3.0]\n    a.length()\n");
    let init = inits
        .get("a")
        .expect("buffer init for 'a' should be present");
    assert_eq!(init.values, vec![1.0, 2.0, 3.0]);
    assert_eq!(init.elem_type, "f32");
}

#[test]
fn test_gpu_let_sized_array_constructor_records_length_and_no_values() {
    let inits = buffer_inits(
        "use system.collections.array\n\nfn main()\n    gpu let a = Array<int, 8>()\n    a.length()\n",
    );
    let init = inits
        .get("a")
        .expect("buffer init for 'a' should be present");
    assert!(init.values.is_empty());
    assert_eq!(init.length, Some(8));
    assert_eq!(init.elem_type, "i32");
}

#[test]
fn test_host_let_is_not_collected() {
    let inits = buffer_inits("fn main()\n    let a = [1, 2, 3, 4]\n    a.length()\n");
    assert!(
        inits.get("a").is_none(),
        "host (non-gpu) bindings must not produce buffer-init metadata"
    );
}
