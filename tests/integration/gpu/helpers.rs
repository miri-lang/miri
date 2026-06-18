// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! WGSL compilation and validation helpers for GPU tests.
//! These functions compile Miri source to WGSL and validate with `naga`.

use miri::ast::statement::StatementKind;
use miri::codegen::backend::Backend;
use miri::codegen::wgsl::{WgslBackend, WgslOptions};
use miri::mir::lowering::lower_function;
use miri::mir::ExecutionModel;
use miri::pipeline::Pipeline;

/// Compile a Miri source with a `forall` or `gpu fn` to WGSL and return the
/// kernel module text. Routes through the real pipeline (`get_mir_bodies_with_rc`,
/// which runs the GpuDevice helper-clone pass) so the emitted module contains
/// exactly the helper functions a real launch would — no test-only divergence.
pub fn compile_to_wgsl(source: &str) -> String {
    let pipeline = Pipeline::new();
    let bodies = pipeline
        .get_gpu_mir_bodies(source)
        .expect("lowering failed");

    // Mirror `build_kernel_registry`: every kernel module also carries the
    // GpuDevice helper bodies reachable from the kernel.
    let mut module_bodies: Vec<(&str, &_)> = bodies
        .iter()
        .filter(|(_, b)| b.execution_model == ExecutionModel::GpuDevice)
        .map(|(n, b)| (n.as_str(), b))
        .collect();
    let kernel = bodies
        .iter()
        .find(|(_, b)| b.execution_model == ExecutionModel::GpuKernel)
        .expect("expected a synthesized GpuKernel body");
    module_bodies.push((kernel.0.as_str(), &kernel.1));

    let artifact = WgslBackend
        .compile(&module_bodies, &WgslOptions::default())
        .expect("WGSL backend should succeed");
    String::from_utf8(artifact.bytes).expect("WGSL output is UTF-8")
}

/// Compile to WGSL and validate with `naga`. Panics if parse or validate fails.
pub fn assert_gpu_wgsl_valid(source: &str) {
    let wgsl = compile_to_wgsl(source);
    let module = naga::front::wgsl::parse_str(&wgsl).unwrap_or_else(|err| {
        panic!(
            "naga parse failed: {}\nWGSL:\n{}",
            err.emit_to_string(&wgsl),
            wgsl
        )
    });
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .unwrap_or_else(|err| panic!("naga validate failed: {:?}\nWGSL:\n{}", err, wgsl));
}

/// Compile a Miri source and extract GPU kernel metadata.
/// Returns a JSON-like value with kernel information (simplified for testing).
pub fn compile_to_manifest(source: &str) -> Result<serde_json::Value, String> {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).map_err(|e| format!("{:?}", e))?;
    let func_stmt = result
        .ast
        .body
        .iter()
        .find(
            |stmt| matches!(&stmt.node, StatementKind::FunctionDeclaration(d) if d.name == "main"),
        )
        .ok_or("source must contain 'fn main'".to_string())?;
    let (_body, lambdas) = lower_function(func_stmt, &result.type_checker, false, false)
        .map_err(|e| format!("{:?}", e))?;

    // Count frame passes
    let mut frame_passes = Vec::new();
    for lambda in &lambdas {
        if lambda.body.execution_model == ExecutionModel::GpuKernel {
            if let Some(backend_md) = &lambda.body.backend_metadata {
                let miri::mir::BackendMetadata::Gpu(gpu_md) = backend_md;
                if gpu_md.is_frame_step {
                    frame_passes.push(serde_json::json!({}));
                }
            }
        }
    }

    Ok(serde_json::json!({
        "framePasses": frame_passes
    }))
}
