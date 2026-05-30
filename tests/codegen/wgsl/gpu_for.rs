// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::statement::StatementKind;
use miri::codegen::backend::Backend;
use miri::codegen::wgsl::{WgslBackend, WgslOptions};
use miri::mir::lowering::lower_function;
use miri::pipeline::Pipeline;

fn synthesize_gpu_for_kernel(source: &str) -> Vec<u8> {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("frontend failed");

    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|stmt| match &stmt.node {
            StatementKind::FunctionDeclaration(decl) => decl.name == "main",
            _ => false,
        })
        .expect("source must contain 'fn main'");

    let (_body, lambdas) =
        lower_function(func_stmt, &result.type_checker, false, false).expect("lowering failed");

    let kernel = lambdas
        .iter()
        .find(|l| l.body.execution_model == miri::mir::ExecutionModel::GpuKernel)
        .expect("expected a synthesized GpuKernel body for `gpu for`");

    let artifact = WgslBackend
        .compile(
            &[(kernel.name.as_str(), &kernel.body)],
            &WgslOptions::default(),
        )
        .expect("wgsl backend should succeed for synthesized kernel");
    artifact.bytes
}

fn assert_wgsl_valid(source: &str) {
    let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|err| {
        panic!(
            "naga failed to parse generated WGSL:\n{}\n--- source ---\n{}",
            err.emit_to_string(source),
            source
        )
    });
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator.validate(&module).unwrap_or_else(|err| {
        panic!(
            "naga failed to validate generated WGSL: {:?}\n--- source ---\n{}",
            err, source
        )
    });
}

#[test]
fn gpu_for_vector_add_emits_naga_valid_wgsl() {
    let bytes = synthesize_gpu_for_kernel(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [5, 6, 7, 8]
    gpu var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = a[i] + b[i]
",
    );
    let source = std::str::from_utf8(&bytes).expect("WGSL output is UTF-8");
    assert_wgsl_valid(source);
}

#[test]
fn gpu_for_emits_compute_attribute_and_bounds_check_if() {
    let bytes = synthesize_gpu_for_kernel(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [5, 6, 7, 8]
    gpu var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = a[i] + b[i]
",
    );
    let source = std::str::from_utf8(&bytes).expect("WGSL output is UTF-8");
    assert!(
        source.contains("@compute"),
        "kernel should be a @compute entry, got:\n{}",
        source
    );
    assert!(
        source.contains("@workgroup_size(256, 1, 1)"),
        "kernel should declare workgroup size 256, got:\n{}",
        source
    );
    assert!(
        source.contains("if (bool("),
        "kernel should emit the structured `if (...) {{ ... }}` bounds check, got:\n{}",
        source
    );
}

#[test]
fn gpu_for_kernel_exposes_capture_buffers_as_storage_bindings() {
    let bytes = synthesize_gpu_for_kernel(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [5, 6, 7, 8]
    gpu var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = a[i] + b[i]
",
    );
    let source = std::str::from_utf8(&bytes).expect("WGSL output is UTF-8");
    let binding_count = source.matches("var<storage, read_write>").count();
    assert_eq!(
        binding_count, 3,
        "expected 3 captured buffers as storage bindings, got:\n{}",
        source
    );
}
